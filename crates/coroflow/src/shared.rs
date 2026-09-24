use std::{
    collections::VecDeque,
    future::Future,
    ops::Deref,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use futures_core::Stream;

use crate::{
    builders::Emitter,
    flow::Flow,
    shaping::EmitterAction,
    state::{StateFlow, SubscriberCount},
    suspending::{Actions, Finish},
    sync::{WakerSet, lock, wake_and_empty},
};

/// What a [`MutableSharedFlow`] does when an emission finds collectors too far
/// behind — Kotlin's `BufferOverflow`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferOverflow {
    /// `emit` waits until the slowest collector makes room; `try_emit` fails.
    Suspend,
    /// The oldest buffered value is dropped; slow collectors skip it.
    DropOldest,
    /// The new value is dropped.
    DropLatest,
}

struct Queued<T> {
    ticket: u64,
    value: Option<T>,
}

struct SharedState<T> {
    buffer: VecDeque<T>,
    head: u64,
    queue: VecDeque<Queued<T>>,
    next_ticket: u64,
    replay_floor: u64,
    cursors: Vec<Option<u64>>,
    collectors: WakerSet,
    emitters: Vec<Waker>,
}

impl<T> SharedState<T> {
    fn buffer_end(&self) -> u64 {
        self.head + self.buffer.len() as u64
    }

    fn slowest(&self) -> Option<u64> {
        self.cursors.iter().flatten().copied().min()
    }

    fn replay_start(&self, replay: usize) -> u64 {
        self.buffer_end()
            .saturating_sub(replay as u64)
            .max(self.head)
            .max(self.replay_floor)
    }

    fn full(&self, capacity: u64) -> bool {
        !self.queue.is_empty()
            || self
                .slowest()
                .is_some_and(|slowest| self.buffer_end() - slowest >= capacity)
    }

    fn trim(&mut self, replay: usize) {
        let keep = self.replay_start(replay);
        let floor = self.slowest().map_or(keep, |slowest| slowest.min(keep));
        while self.head < floor && self.buffer.pop_front().is_some() {
            self.head += 1;
        }
    }

    fn drain_queue(&mut self, capacity: u64) -> bool {
        let mut moved = false;
        while let Some(front) = self.queue.front() {
            let room = match self.slowest() {
                None => true,
                Some(slowest) if capacity == 0 => slowest > self.head,
                Some(slowest) => self.buffer_end() - slowest < capacity,
            };
            if !room && front.value.is_some() {
                break;
            }
            let Some(queued) = self.queue.pop_front() else {
                break;
            };
            moved = true;
            if capacity == 0 {
                self.head += 1;
            } else if let Some(value) = queued.value {
                self.buffer.push_back(value);
            }
        }
        moved
    }

    fn settle(&mut self, capacity: u64, replay: usize) -> Vec<Waker> {
        let moved = self.drain_queue(capacity);
        self.trim(replay);
        if !moved {
            return Vec::new();
        }
        let mut wakers = std::mem::take(&mut self.emitters);
        wakers.extend(self.collectors.take_wakers());
        wakers
    }
}

enum Offer {
    Delivered(Vec<Waker>),
    Rejected,
    Queued(u64, Vec<Waker>),
}

pub(crate) struct SharedInner<T> {
    state: Mutex<SharedState<T>>,
    subscribers: SubscriberCount,
    replay: usize,
    capacity: u64,
    overflow: BufferOverflow,
}

impl<T> SharedInner<T> {
    fn offer(&self, value: T, may_wait: bool) -> Offer {
        let mut state = lock(&self.state);
        let collected = state.slowest().is_some();
        if !collected && self.replay == 0 {
            return Offer::Delivered(Vec::new());
        }
        if collected && state.full(self.capacity) {
            match self.overflow {
                BufferOverflow::DropLatest => return Offer::Delivered(Vec::new()),
                BufferOverflow::Suspend if !may_wait => return Offer::Rejected,
                BufferOverflow::Suspend => {
                    let ticket = state.next_ticket;
                    state.next_ticket += 1;
                    state.queue.push_back(Queued {
                        ticket,
                        value: Some(value),
                    });
                    let wakers = if self.capacity == 0 {
                        state.collectors.take_wakers()
                    } else {
                        Vec::new()
                    };
                    return Offer::Queued(ticket, wakers);
                }
                BufferOverflow::DropOldest => {
                    if state.buffer.pop_front().is_some() {
                        state.head += 1;
                    }
                }
            }
        }
        state.buffer.push_back(value);
        state.trim(self.replay);
        Offer::Delivered(state.collectors.take_wakers())
    }

    fn wake(&self, wakers: Vec<Waker>) {
        let emptied = wake_and_empty(wakers);
        lock(&self.state).collectors.recycle(emptied);
    }

    pub(crate) fn offer_waiting(&self, value: T) -> Option<u64> {
        match self.offer(value, true) {
            Offer::Delivered(wakers) => {
                self.wake(wakers);
                None
            }
            Offer::Rejected => None,
            Offer::Queued(ticket, wakers) => {
                self.wake(wakers);
                Some(ticket)
            }
        }
    }

    pub(crate) fn poll_queued(&self, ticket: u64, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = lock(&self.state);
        let waiting = state
            .queue
            .front()
            .is_some_and(|front| front.ticket <= ticket);
        if !waiting {
            return Poll::Ready(());
        }
        if !state
            .emitters
            .iter()
            .any(|waker| waker.will_wake(cx.waker()))
        {
            state.emitters.push(cx.waker().clone());
        }
        Poll::Pending
    }

    pub(crate) fn withdraw(&self, ticket: u64) {
        let wakers = {
            let mut state = lock(&self.state);
            let Ok(index) = state
                .queue
                .binary_search_by_key(&ticket, |queued| queued.ticket)
            else {
                return;
            };
            if self.capacity == 0 && index == 0 {
                if let Some(front) = state.queue.front_mut() {
                    front.value = None;
                }
            } else {
                state.queue.remove(index);
            }
            state.settle(self.capacity, self.replay)
        };
        self.wake(wakers);
    }

    pub(crate) fn reset_replay_cache(&self) {
        let mut state = lock(&self.state);
        state.replay_floor = state.buffer_end();
        state.trim(self.replay);
    }
}

/// A hot broadcast of values to every current collector — Kotlin's
/// `MutableSharedFlow`.
///
/// It keeps the last `replay` values for new collectors and lets the slowest
/// collector fall up to `replay + extra_capacity` values behind; what happens
/// beyond that is its [`BufferOverflow`]. A value waiting for room is not
/// visible to any collector yet. With no collectors, emitting never waits and
/// only the replay values are kept.
pub struct MutableSharedFlow<T> {
    flow: SharedFlow<T>,
}

impl<T> Clone for MutableSharedFlow<T> {
    fn clone(&self) -> Self {
        Self {
            flow: self.flow.clone(),
        }
    }
}

impl<T> Deref for MutableSharedFlow<T> {
    type Target = SharedFlow<T>;

    fn deref(&self) -> &SharedFlow<T> {
        &self.flow
    }
}

impl<T: Clone> MutableSharedFlow<T> {
    /// A shared flow whose `emit` waits for slow collectors — Kotlin's
    /// `MutableSharedFlow(replay, extraBufferCapacity)`.
    pub fn new(replay: usize, extra_capacity: usize) -> Self {
        Self::with_overflow(replay, extra_capacity, BufferOverflow::Suspend)
    }

    /// A shared flow with an explicit overflow policy.
    pub fn with_overflow(replay: usize, extra_capacity: usize, overflow: BufferOverflow) -> Self {
        let capacity = (replay + extra_capacity) as u64;
        let capacity = if overflow == BufferOverflow::Suspend {
            capacity
        } else {
            capacity.max(1)
        };
        let inner = Arc::new(SharedInner {
            state: Mutex::new(SharedState {
                buffer: VecDeque::with_capacity(replay + extra_capacity),
                head: 0,
                queue: VecDeque::new(),
                next_ticket: 0,
                replay_floor: 0,
                cursors: Vec::new(),
                collectors: WakerSet::default(),
                emitters: Vec::new(),
            }),
            subscribers: SubscriberCount::default(),
            replay,
            capacity,
            overflow,
        });
        Self {
            flow: SharedFlow { inner },
        }
    }

    /// Emits `value`, waiting under [`BufferOverflow::Suspend`] while the
    /// slowest collector is too far behind — Kotlin's `emit`. Dropping the
    /// future while it waits withdraws the value.
    pub fn emit(&self, value: T) -> EmitShared<'_, T> {
        EmitShared {
            flow: self,
            value: Some(value),
            ticket: None,
        }
    }

    /// Emits `value` if that needs no waiting and reports whether it did —
    /// Kotlin's `tryEmit`. Under [`BufferOverflow::DropOldest`] and
    /// [`BufferOverflow::DropLatest`] it always succeeds, even when the
    /// policy drops a value.
    pub fn try_emit(&self, value: T) -> bool {
        match self.flow.inner.offer(value, false) {
            Offer::Delivered(wakers) | Offer::Queued(_, wakers) => {
                self.flow.inner.wake(wakers);
                true
            }
            Offer::Rejected => false,
        }
    }

    /// Forgets the replayed values, so new collectors see only what is
    /// emitted from now on — Kotlin's `resetReplayCache`. Current collectors
    /// keep what is buffered for them.
    pub fn reset_replay_cache(&self) {
        self.flow.inner.reset_replay_cache();
    }

    /// A read-only view — Kotlin's `asSharedFlow()`.
    pub fn as_shared_flow(&self) -> SharedFlow<T> {
        self.flow.clone()
    }

    pub(crate) fn inner(&self) -> &SharedInner<T> {
        &self.flow.inner
    }
}

/// The future returned by [`MutableSharedFlow::emit`].
#[must_use = "the value is emitted only when the future is awaited"]
pub struct EmitShared<'a, T> {
    flow: &'a MutableSharedFlow<T>,
    value: Option<T>,
    ticket: Option<u64>,
}

impl<T> Unpin for EmitShared<'_, T> {}

impl<T> Future for EmitShared<'_, T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let inner = &this.flow.flow.inner;
        if let Some(value) = this.value.take() {
            this.ticket = inner.offer_waiting(value);
        }
        let Some(ticket) = this.ticket else {
            return Poll::Ready(());
        };
        let polled = inner.poll_queued(ticket, cx);
        if polled.is_ready() {
            this.ticket = None;
        }
        polled
    }
}

impl<T> Drop for EmitShared<'_, T> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket {
            self.flow.flow.inner.withdraw(ticket);
        }
    }
}

/// The read-only side of a [`MutableSharedFlow`] — Kotlin's `SharedFlow`.
///
/// A [`MutableSharedFlow`] dereferences to it, the way Kotlin's
/// `MutableSharedFlow` extends `SharedFlow`.
pub struct SharedFlow<T> {
    inner: Arc<SharedInner<T>>,
}

impl<T> Clone for SharedFlow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> SharedFlow<T> {
    /// How many collectors are running, as a state flow of its own — Kotlin's
    /// `subscriptionCount`.
    pub fn subscription_count(&self) -> StateFlow<usize> {
        self.inner.subscribers.as_flow()
    }

    pub(crate) fn subscribers(&self) -> &SubscriberCount {
        &self.inner.subscribers
    }
}

impl<T: Clone> SharedFlow<T> {
    /// The values a new collector would be replayed right now — Kotlin's
    /// `replayCache`.
    pub fn replay_cache(&self) -> Vec<T> {
        let state = lock(&self.inner.state);
        let start = (state.replay_start(self.inner.replay) - state.head) as usize;
        state.buffer.range(start..).cloned().collect()
    }

    /// Runs the suspending `action` for each collector once its subscription
    /// is registered and before it receives shared values; what `action`
    /// emits goes to that collector only — Kotlin's `onSubscription`.
    pub fn on_subscription<A, Fut>(&self, action: A) -> OnSubscription<T, A>
    where
        A: FnOnce(Emitter<T>) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        OnSubscription {
            flow: self.clone(),
            action,
        }
    }
}

/// One subscription to a [`SharedFlow`]; dropping it unsubscribes.
pub struct SharedRun<T> {
    inner: Arc<SharedInner<T>>,
    key: usize,
}

fn open_shared<T>(inner: &Arc<SharedInner<T>>) -> SharedRun<T> {
    inner.subscribers.opened();
    let mut state = lock(&inner.state);
    let cursor = state.replay_start(inner.replay);
    let key = state.collectors.insert();
    if state.cursors.len() <= key {
        state.cursors.resize(key + 1, None);
    }
    state.cursors[key] = Some(cursor);
    SharedRun {
        inner: Arc::clone(inner),
        key,
    }
}

impl<T: Clone> Flow for SharedFlow<T> {
    type Item = T;
    type Run = SharedRun<T>;

    fn open(&self) -> SharedRun<T> {
        open_shared(&self.inner)
    }
}

impl<T: Clone> Flow for MutableSharedFlow<T> {
    type Item = T;
    type Run = SharedRun<T>;

    fn open(&self) -> SharedRun<T> {
        self.flow.open()
    }
}

impl<T> Unpin for SharedRun<T> {}

enum Read<T> {
    Value(T),
    Withdrawn,
    Nothing,
}

impl<T: Clone> SharedRun<T> {
    fn read(&self, state: &mut SharedState<T>, cx: &mut Context<'_>) -> Option<Read<T>> {
        let cursor = state.cursors.get(self.key).copied().flatten()?;
        let cursor = cursor.max(state.head);
        let read = if cursor < state.buffer_end() {
            state
                .buffer
                .get((cursor - state.head) as usize)
                .cloned()
                .map_or(Read::Nothing, Read::Value)
        } else if self.inner.capacity == 0 && cursor == state.head {
            match state.queue.front() {
                Some(queued) => queued.value.clone().map_or(Read::Withdrawn, Read::Value),
                None => Read::Nothing,
            }
        } else {
            Read::Nothing
        };
        let next = match read {
            Read::Nothing => {
                state.collectors.register(self.key, cx.waker());
                cursor
            }
            Read::Value(_) | Read::Withdrawn => cursor + 1,
        };
        if let Some(slot) = state.cursors.get_mut(self.key) {
            *slot = Some(next);
        }
        Some(read)
    }
}

impl<T: Clone> Stream for SharedRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        loop {
            let (read, wakers) = {
                let mut state = lock(&this.inner.state);
                let Some(read) = this.read(&mut state, cx) else {
                    return Poll::Ready(None);
                };
                let wakers = match read {
                    Read::Nothing => Vec::new(),
                    Read::Value(_) | Read::Withdrawn => {
                        state.settle(this.inner.capacity, this.inner.replay)
                    }
                };
                (read, wakers)
            };
            this.inner.wake(wakers);
            match read {
                Read::Value(value) => return Poll::Ready(Some(value)),
                Read::Withdrawn => {}
                Read::Nothing => return Poll::Pending,
            }
        }
    }
}

impl<T> Drop for SharedRun<T> {
    fn drop(&mut self) {
        let wakers = {
            let mut state = lock(&self.inner.state);
            state.collectors.remove(self.key);
            if let Some(cursor) = state.cursors.get_mut(self.key) {
                *cursor = None;
            }
            state.settle(self.inner.capacity, self.inner.replay)
        };
        self.inner.wake(wakers);
        self.inner.subscribers.closed();
    }
}

/// The flow returned by [`SharedFlow::on_subscription`].
pub struct OnSubscription<T, A> {
    flow: SharedFlow<T>,
    action: A,
}

impl<T, A: Clone> Clone for OnSubscription<T, A> {
    fn clone(&self) -> Self {
        Self {
            flow: self.flow.clone(),
            action: self.action.clone(),
        }
    }
}

/// One run of an [`OnSubscription`].
pub struct OnSubscriptionRun<T, A>
where
    EmitterAction<T>: crate::suspending::Step<(), A>,
{
    run: SharedRun<T>,
    prelude: Actions<(), A, EmitterAction<T>>,
}

impl<T, A> Unpin for OnSubscriptionRun<T, A> where EmitterAction<T>: crate::suspending::Step<(), A> {}

impl<T, A, Fut> Flow for OnSubscription<T, A>
where
    T: Clone,
    A: FnOnce(Emitter<T>) -> Fut + Clone,
    Fut: Future<Output = ()>,
{
    type Item = T;
    type Run = OnSubscriptionRun<T, A>;

    fn open(&self) -> Self::Run {
        let run = self.flow.open();
        let mut prelude = Actions::new(self.action.clone());
        prelude.start(());
        OnSubscriptionRun { run, prelude }
    }
}

impl<T, A, Fut> Stream for OnSubscriptionRun<T, A>
where
    T: Clone,
    A: FnOnce(Emitter<T>) -> Fut + Clone,
    Fut: Future<Output = ()>,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        match this.prelude.poll(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Finish::Emit(value))) => return Poll::Ready(Some(value)),
            Poll::Ready(Some(Finish::Skip | Finish::Stop) | None) => {}
        }
        Pin::new(&mut this.run).poll_next(cx)
    }
}
