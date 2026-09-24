use std::{
    collections::VecDeque,
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use futures_core::Stream;

use crate::{
    dispatcher::{Dispatchers, current_dispatcher},
    flow::Flow,
    job::Job,
    scope::{CoroutineScope, ScopeHandle, Spawn},
    sync::lock,
};

/// How many values a channel holds before `send` waits — Kotlin's channel
/// capacities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capacity {
    /// `send` waits until a receiver takes the value.
    Rendezvous,
    /// Up to this many values wait in the channel.
    Buffered(usize),
    /// `send` never waits.
    Unlimited,
    /// Only the newest value is kept; `send` never waits.
    Conflated,
}

impl Capacity {
    /// Kotlin's `Channel.BUFFERED`: 64 values.
    pub const BUFFERED: Capacity = Capacity::Buffered(64);
}

/// The channel was closed, or every receiver is gone; the value comes back.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SendError<T>(pub T);

impl<T> fmt::Debug for SendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SendError(..)")
    }
}

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the channel is closed")
    }
}

impl<T> std::error::Error for SendError<T> {}

/// Why [`Sender::try_send`] could not send; the value comes back.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// The channel has no room right now.
    Full(T),
    /// The channel was closed, or every receiver is gone.
    Closed(T),
}

impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrySendError::Full(_) => formatter.write_str("Full(..)"),
            TrySendError::Closed(_) => formatter.write_str("Closed(..)"),
        }
    }
}

impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrySendError::Full(_) => formatter.write_str("the channel is full"),
            TrySendError::Closed(_) => formatter.write_str("the channel is closed"),
        }
    }
}

impl<T> std::error::Error for TrySendError<T> {}

/// Why [`Receiver::try_recv`] returned no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TryRecvError {
    /// Nothing is waiting in the channel right now.
    #[error("the channel is empty")]
    Empty,
    /// The channel is closed and drained.
    #[error("the channel is closed")]
    Closed,
}

struct ChannelState<T> {
    queue: VecDeque<T>,
    capacity: Capacity,
    received: u64,
    sent: u64,
    senders: usize,
    receivers: usize,
    closed: bool,
    send_waiters: Vec<Waker>,
    recv_waiters: Vec<Waker>,
}

impl<T> ChannelState<T> {
    fn is_open(&self) -> bool {
        !self.closed && self.receivers > 0
    }

    fn push(&mut self, value: T) -> (u64, Vec<Waker>) {
        let sequence = self.sent;
        self.sent += 1;
        self.queue.push_back(value);
        (sequence, std::mem::take(&mut self.recv_waiters))
    }

    fn room(&self) -> bool {
        match self.capacity {
            Capacity::Unlimited | Capacity::Conflated => true,
            Capacity::Buffered(limit) => self.queue.len() < limit.max(1),
            Capacity::Rendezvous => self.queue.is_empty(),
        }
    }
}

fn wait_on(waiters: &mut Vec<Waker>, cx: &Context<'_>) {
    if !waiters.iter().any(|waiter| waiter.will_wake(cx.waker())) {
        waiters.push(cx.waker().clone());
    }
}

fn wake(waiters: Vec<Waker>) {
    for waiter in waiters {
        waiter.wake();
    }
}

type Shared<T> = Arc<Mutex<ChannelState<T>>>;

/// Creates a channel — Kotlin's `Channel(capacity)`.
///
/// Both ends are cheap to clone. Each value goes to exactly one receiver. The
/// channel closes when [`Sender::close`] is called or every sender is
/// dropped; receivers then drain what is left.
pub fn channel<T>(capacity: Capacity) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Mutex::new(ChannelState {
        queue: VecDeque::new(),
        capacity,
        received: 0,
        sent: 0,
        senders: 1,
        receivers: 1,
        closed: false,
        send_waiters: Vec::new(),
        recv_waiters: Vec::new(),
    }));
    (
        Sender {
            shared: Arc::clone(&shared),
        },
        Receiver { shared },
    )
}

/// The sending end of a [`channel`].
pub struct Sender<T> {
    shared: Shared<T>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        lock(&self.shared).senders += 1;
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let waiters = {
            let mut state = lock(&self.shared);
            state.senders -= 1;
            if state.senders > 0 {
                return;
            }
            state.closed = true;
            std::mem::take(&mut state.recv_waiters)
        };
        wake(waiters);
    }
}

impl<T> Sender<T> {
    /// Sends `value`, waiting while the channel has no room.
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        SendFuture {
            sender: self,
            value: Some(value),
            handed_over: None,
        }
    }

    /// Sends `value` if there is room right now.
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        let (evicted, waiters) = {
            let mut state = lock(&self.shared);
            if !state.is_open() {
                return Err(TrySendError::Closed(value));
            }
            let evicted = match state.capacity {
                Capacity::Conflated => state.queue.pop_front(),
                Capacity::Rendezvous if state.recv_waiters.is_empty() => {
                    return Err(TrySendError::Full(value));
                }
                _ if !state.room() => return Err(TrySendError::Full(value)),
                _ => None,
            };
            (evicted, state.push(value).1)
        };
        drop(evicted);
        wake(waiters);
        Ok(())
    }

    /// Closes the channel; receivers still get the values already sent.
    pub fn close(&self) {
        let (receivers, senders) = {
            let mut state = lock(&self.shared);
            state.closed = true;
            (
                std::mem::take(&mut state.recv_waiters),
                std::mem::take(&mut state.send_waiters),
            )
        };
        wake(receivers);
        wake(senders);
    }

    /// Whether the channel no longer accepts values.
    pub fn is_closed(&self) -> bool {
        !lock(&self.shared).is_open()
    }

    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = lock(&self.shared);
        if !state.is_open() {
            return Poll::Ready(());
        }
        wait_on(&mut state.send_waiters, cx);
        Poll::Pending
    }
}

/// The future returned by [`Sender::send`].
pub struct SendFuture<'a, T> {
    sender: &'a Sender<T>,
    value: Option<T>,
    handed_over: Option<u64>,
}

impl<T> Unpin for SendFuture<'_, T> {}

impl<T> Future for SendFuture<'_, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut state = lock(&this.sender.shared);
        if let Some(sequence) = this.handed_over {
            if state.received > sequence {
                return Poll::Ready(Ok(()));
            }
            if state.receivers == 0
                && let Some(value) = state.queue.pop_back()
            {
                return Poll::Ready(Err(SendError(value)));
            }
            wait_on(&mut state.send_waiters, cx);
            return Poll::Pending;
        }
        let Some(value) = this.value.take() else {
            return Poll::Ready(Ok(()));
        };
        if !state.is_open() {
            return Poll::Ready(Err(SendError(value)));
        }
        if !state.room() {
            this.value = Some(value);
            wait_on(&mut state.send_waiters, cx);
            return Poll::Pending;
        }
        let evicted = match state.capacity {
            Capacity::Conflated => state.queue.pop_front(),
            _ => None,
        };
        let (sequence, receivers) = state.push(value);
        let rendezvous = state.capacity == Capacity::Rendezvous;
        if rendezvous {
            this.handed_over = Some(sequence);
            wait_on(&mut state.send_waiters, cx);
        }
        drop(state);
        drop(evicted);
        wake(receivers);
        if rendezvous {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

/// The receiving end of a [`channel`]; it is also a hot [`Flow`] whose
/// collectors share the values — Kotlin's `receiveAsFlow()`.
pub struct Receiver<T> {
    shared: Shared<T>,
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        lock(&self.shared).receivers += 1;
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        let waiters = {
            let mut state = lock(&self.shared);
            state.receivers -= 1;
            if state.receivers > 0 {
                return;
            }
            std::mem::take(&mut state.send_waiters)
        };
        wake(waiters);
    }
}

impl<T> Receiver<T> {
    /// Waits for the next value; `None` once the channel is closed and empty.
    pub fn recv(&self) -> RecvFuture<'_, T> {
        RecvFuture { receiver: self }
    }

    /// Takes the next value if one is waiting.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let (value, waiters) = {
            let mut state = lock(&self.shared);
            match state.queue.pop_front() {
                Some(value) => {
                    state.received += 1;
                    (value, std::mem::take(&mut state.send_waiters))
                }
                None if state.closed => return Err(TryRecvError::Closed),
                None => return Err(TryRecvError::Empty),
            }
        };
        wake(waiters);
        Ok(value)
    }

    fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let (value, waiters) = {
            let mut state = lock(&self.shared);
            match state.queue.pop_front() {
                Some(value) => {
                    state.received += 1;
                    (value, std::mem::take(&mut state.send_waiters))
                }
                None if state.closed => return Poll::Ready(None),
                None => {
                    wait_on(&mut state.recv_waiters, cx);
                    return Poll::Pending;
                }
            }
        };
        wake(waiters);
        Poll::Ready(Some(value))
    }
}

/// The future returned by [`Receiver::recv`].
pub struct RecvFuture<'a, T> {
    receiver: &'a Receiver<T>,
}

impl<T> Future for RecvFuture<'_, T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.receiver.poll_recv(cx)
    }
}

impl<T> Flow for Receiver<T> {
    type Item = T;
    type Run = Receiver<T>;

    fn open(&self) -> Receiver<T> {
        self.clone()
    }
}

impl<T> Stream for Receiver<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.poll_recv(cx)
    }
}

/// Launches `block` in `scope` with the sending end of a new channel and
/// returns the receiving end — Kotlin's `produce`. The channel closes when
/// `block` and every sender it handed out are done.
pub fn produce<S, T, F, Fut>(scope: &S, capacity: Capacity, block: F) -> Receiver<T>
where
    S: Spawn<Fut>,
    F: FnOnce(Sender<T>) -> Fut,
    Fut: Future<Output = ()>,
{
    let (sender, receiver) = channel(capacity);
    scope.spawn(block(sender));
    receiver
}

/// The sending side handed to a [`channel_flow`] block.
pub struct Producer<T> {
    sender: Sender<T>,
    scope: ScopeHandle,
}

impl<T> Clone for Producer<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            scope: self.scope.clone(),
        }
    }
}

impl<T: Send + 'static> Producer<T> {
    /// Sends `value` to the collector, waiting while its buffer is full.
    pub fn send(&self, value: T) -> SendFuture<'_, T> {
        self.sender.send(value)
    }

    /// Sends `value` if there is room right now — what a callback calls.
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        self.sender.try_send(value)
    }

    /// A sender for code that outlives this borrow, such as a callback.
    pub fn sender(&self) -> Sender<T> {
        self.sender.clone()
    }

    /// Runs `future` concurrently with the block, cancelled with the
    /// collection — producers launched this way may all send.
    pub fn launch<F>(&self, future: F) -> Job
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.scope.launch(future)
    }

    /// Waits until the collector stops, then runs `on_close` — Kotlin's
    /// `awaitClose`. `on_close` also runs when the collection is cancelled,
    /// which is where a callback registration is undone.
    pub async fn await_close(self, on_close: impl FnOnce()) {
        let _cleanup = OnDrop(Some(on_close));
        std::future::poll_fn(|cx| self.sender.poll_closed(cx)).await;
    }
}

struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

/// A cold flow whose values a coroutine sends through a channel — Kotlin's
/// `channelFlow`.
///
/// Every collection runs `block` on the collector's dispatcher with a
/// [`Producer`] that may send from several concurrent coroutines. The run ends
/// once `block` and everything it launched are done; cancelling the collection
/// cancels them all.
pub fn channel_flow<T, F, Fut>(block: F) -> ChannelFlow<T, F>
where
    F: Fn(Producer<T>) -> Fut + Clone,
    Fut: Future<Output = ()> + Send + 'static,
    T: Send + 'static,
{
    ChannelFlow {
        block,
        _item: PhantomData,
    }
}

pub use channel_flow as callback_flow;

/// The flow returned by [`channel_flow`].
pub struct ChannelFlow<T, F> {
    block: F,
    _item: PhantomData<fn() -> T>,
}

impl<T, F: Clone> Clone for ChannelFlow<T, F> {
    fn clone(&self) -> Self {
        Self {
            block: self.block.clone(),
            _item: PhantomData,
        }
    }
}

/// One run of a [`ChannelFlow`]; dropping it cancels its producers.
///
/// The producer starts on the first poll, so it runs on the dispatcher of
/// the coroutine that collects.
pub struct ChannelFlowRun<T, F> {
    receiver: Receiver<T>,
    sender: Option<Sender<T>>,
    block: Option<F>,
    scope: Option<CoroutineScope>,
}

impl<T, F> Unpin for ChannelFlowRun<T, F> {}

impl<T, F, Fut> Flow for ChannelFlow<T, F>
where
    F: Fn(Producer<T>) -> Fut + Clone,
    Fut: Future<Output = ()> + Send + 'static,
    T: Send + 'static,
{
    type Item = T;
    type Run = ChannelFlowRun<T, F>;

    fn open(&self) -> Self::Run {
        let (sender, receiver) = channel(Capacity::BUFFERED);
        ChannelFlowRun {
            receiver,
            sender: Some(sender),
            block: Some(self.block.clone()),
            scope: None,
        }
    }
}

impl<T, F, Fut> Stream for ChannelFlowRun<T, F>
where
    F: Fn(Producer<T>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
    T: Send + 'static,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        if let (Some(block), Some(sender)) = (this.block.take(), this.sender.take()) {
            let dispatcher = current_dispatcher().unwrap_or_else(Dispatchers::default_pool);
            let scope = CoroutineScope::new(dispatcher);
            let producer = Producer {
                sender,
                scope: scope.handle(),
            };
            scope.launch(block(producer));
            this.scope = Some(scope);
        }
        this.receiver.poll_recv(cx)
    }
}
