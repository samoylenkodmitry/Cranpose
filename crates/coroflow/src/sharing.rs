use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    clock::Timer,
    flow::{BoxFlow, Flow},
    shared::{BufferOverflow, MutableSharedFlow, SharedFlow},
    state::{StateFlow, StateShared, SubscriberCount},
    sync::{OneshotReceiver, OneshotSender, oneshot},
};

/// The smallest buffer [`share_in`](crate::FlowExt::share_in) gives
/// collectors that fall behind, replay included — Kotlin's default channel
/// size. Beyond it the upstream waits.
pub const SHARE_IN_BUFFER: usize = 64;

/// What a custom [`SharingStarted`] strategy tells the sharing coroutine —
/// Kotlin's `SharingCommand`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharingCommand {
    /// Run the upstream.
    Start,
    /// Stop the upstream and keep the replayed values.
    Stop,
    /// Stop the upstream and forget the replayed values; a
    /// [`state_in`](crate::FlowExt::state_in) state goes back to its initial
    /// value.
    StopAndResetReplayCache,
}

type CommandFactory = Arc<dyn Fn(StateFlow<usize>) -> BoxFlow<SharingCommand> + Send + Sync>;

/// A strategy made by [`SharingStarted::custom`].
#[derive(Clone)]
pub struct CustomSharing {
    commands: CommandFactory,
}

impl fmt::Debug for CustomSharing {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CustomSharing")
    }
}

/// When a shared flow runs its upstream — Kotlin's `SharingStarted`.
#[derive(Clone, Debug)]
pub enum SharingStarted {
    /// Starts at once and never stops.
    Eagerly,
    /// Starts with the first collector and never stops.
    Lazily,
    /// Runs while anyone collects, and stops `stop_timeout` after the last
    /// collector leaves. A collector that returns within the timeout finds the
    /// upstream still running.
    WhileSubscribed {
        /// How long to keep running with no collectors.
        stop_timeout: Duration,
        /// How long after stopping the replayed values are forgotten; `None`
        /// keeps them.
        replay_expiration: Option<Duration>,
    },
    /// Follows the commands of a flow over the collector count.
    Custom(CustomSharing),
}

impl SharingStarted {
    /// Kotlin's `WhileSubscribed(stopTimeout)`: stops `stop_timeout` after the
    /// last collector leaves and keeps the replayed values.
    pub const fn while_subscribed(stop_timeout: Duration) -> Self {
        Self::WhileSubscribed {
            stop_timeout,
            replay_expiration: None,
        }
    }

    /// Forgets the replayed values once the upstream has been stopped for
    /// `after`, so a [`state_in`](crate::FlowExt::state_in) state goes back to
    /// its initial value — Kotlin's `replayExpiration`. Only
    /// [`SharingStarted::WhileSubscribed`] stops, so the others ignore it.
    #[must_use]
    pub fn replay_expiration(self, after: Duration) -> Self {
        match self {
            Self::WhileSubscribed { stop_timeout, .. } => Self::WhileSubscribed {
                stop_timeout,
                replay_expiration: Some(after),
            },
            other => other,
        }
    }

    /// A strategy of your own: `commands` turns the collector count into
    /// [`SharingCommand`]s — Kotlin's custom `SharingStarted`.
    pub fn custom(
        commands: impl Fn(StateFlow<usize>) -> BoxFlow<SharingCommand> + Send + Sync + 'static,
    ) -> Self {
        Self::Custom(CustomSharing {
            commands: Arc::new(commands),
        })
    }
}

pub(crate) trait ShareTarget<T> {
    fn offer(&self, value: T) -> Option<u64>;
    fn poll_room(&self, ticket: u64, cx: &mut Context<'_>) -> Poll<()>;
    fn withdraw(&self, ticket: u64);
    fn reset(&self, initial: Option<&T>);
}

impl<T: Clone + PartialEq> ShareTarget<T> for StateFlow<T> {
    fn offer(&self, value: T) -> Option<u64> {
        self.publish(value);
        None
    }

    fn poll_room(&self, _: u64, _: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(())
    }

    fn withdraw(&self, _: u64) {}

    fn reset(&self, initial: Option<&T>) {
        if let Some(initial) = initial {
            self.publish(initial.clone());
        }
    }
}

impl<T: Clone> ShareTarget<T> for MutableSharedFlow<T> {
    fn offer(&self, value: T) -> Option<u64> {
        self.inner().offer_waiting(value)
    }

    fn poll_room(&self, ticket: u64, cx: &mut Context<'_>) -> Poll<()> {
        self.inner().poll_queued(ticket, cx)
    }

    fn withdraw(&self, ticket: u64) {
        self.inner().withdraw(ticket);
    }

    fn reset(&self, _: Option<&T>) {
        self.reset_replay_cache();
    }
}

type CommandRun = Pin<Box<dyn Stream<Item = SharingCommand> + Send>>;

/// The coroutine behind [`state_in`](crate::FlowExt::state_in) and
/// [`share_in`](crate::FlowExt::share_in): it watches the collector count of
/// `target` and starts or stops the upstream.
pub struct SharingTask<F: Flow, S> {
    upstream: F,
    started: SharingStarted,
    target: S,
    subscribers: SubscriberCount,
    counter_key: usize,
    counter_seen: Option<u64>,
    current: usize,
    opened_seen: u64,
    newly_subscribed: bool,
    ever_subscribed: bool,
    commands: Option<CommandRun>,
    commanded: bool,
    run: Option<F::Run>,
    finished: bool,
    waiting: Option<u64>,
    stop_timer: Timer,
    expiry_timer: Timer,
    initial: Option<F::Item>,
}

impl<F: Flow, S> Unpin for SharingTask<F, S> {}

impl<F: Flow, S> SharingTask<F, S> {
    fn new(
        upstream: F,
        started: SharingStarted,
        target: S,
        subscribers: SubscriberCount,
        initial: Option<F::Item>,
    ) -> Self {
        let counter_key = subscribers.watch().subscribe();
        let commands = match &started {
            SharingStarted::Custom(custom) => Some((custom.commands)(subscribers.as_flow()).open()),
            _ => None,
        };
        Self {
            upstream,
            started,
            target,
            subscribers,
            counter_key,
            counter_seen: None,
            current: 0,
            opened_seen: 0,
            newly_subscribed: false,
            ever_subscribed: false,
            commands,
            commanded: false,
            run: None,
            finished: false,
            waiting: None,
            stop_timer: Timer::default(),
            expiry_timer: Timer::default(),
            initial,
        }
    }

    fn observe_subscribers(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(current) = self.subscribers.watch().poll_changed(
            self.counter_key,
            &mut self.counter_seen,
            cx,
            |count| *count,
        ) {
            self.current = current;
            let opened = self.subscribers.opened_total();
            if opened > self.opened_seen {
                self.opened_seen = opened;
                self.newly_subscribed = true;
            }
        }
    }
}

impl<F: Flow, S> SharingTask<F, S> {
    fn follow_commands(&mut self, cx: &mut Context<'_>) -> bool
    where
        S: ShareTarget<F::Item>,
    {
        while let Some(commands) = self.commands.as_mut() {
            match commands.as_mut().poll_next(cx) {
                Poll::Ready(Some(SharingCommand::Start)) => self.commanded = true,
                Poll::Ready(Some(SharingCommand::Stop)) => self.commanded = false,
                Poll::Ready(Some(SharingCommand::StopAndResetReplayCache)) => {
                    self.commanded = false;
                    self.stop_upstream();
                    self.target.reset(self.initial.as_ref());
                }
                Poll::Ready(None) => self.commands = None,
                Poll::Pending => break,
            }
        }
        self.commanded
    }

    fn wants_upstream(&mut self, cx: &mut Context<'_>) -> bool
    where
        S: ShareTarget<F::Item>,
    {
        let newly_subscribed = std::mem::take(&mut self.newly_subscribed);
        match self.started {
            SharingStarted::Eagerly => true,
            SharingStarted::Lazily => {
                self.ever_subscribed |= newly_subscribed;
                self.ever_subscribed
            }
            SharingStarted::Custom(_) => self.follow_commands(cx),
            SharingStarted::WhileSubscribed { stop_timeout, .. } => {
                if self.current > 0 {
                    self.stop_timer.cancel();
                    return true;
                }
                if self.run.is_none() && !newly_subscribed {
                    return false;
                }
                if newly_subscribed || !self.stop_timer.is_armed() {
                    self.stop_timer.start(stop_timeout);
                }
                self.stop_timer.poll_elapsed(cx).is_pending()
            }
        }
    }

    fn stop_upstream(&mut self)
    where
        S: ShareTarget<F::Item>,
    {
        if let Some(ticket) = self.waiting.take() {
            self.target.withdraw(ticket);
        }
        if self.run.take().is_some()
            && let SharingStarted::WhileSubscribed {
                replay_expiration: Some(after),
                ..
            } = self.started
        {
            self.expiry_timer.start(after);
        }
        self.finished = false;
        self.stop_timer.cancel();
    }

    fn poll_sharing(&mut self, cx: &mut Context<'_>) -> Poll<()>
    where
        S: ShareTarget<F::Item>,
    {
        self.observe_subscribers(cx);
        if !self.wants_upstream(cx) {
            self.stop_upstream();
            if self.expiry_timer.poll_elapsed(cx).is_ready() {
                self.target.reset(self.initial.as_ref());
            }
            return Poll::Pending;
        }
        self.expiry_timer.cancel();
        if self.run.is_none() && !self.finished {
            self.run = Some(self.upstream.open());
        }
        loop {
            if let Some(ticket) = self.waiting {
                if self.target.poll_room(ticket, cx).is_pending() {
                    return Poll::Pending;
                }
                self.waiting = None;
            }
            let Some(run) = self.run.as_mut() else {
                return Poll::Pending;
            };
            match Pin::new(run).poll_next(cx) {
                Poll::Ready(Some(value)) => self.waiting = self.target.offer(value),
                Poll::Ready(None) => {
                    self.run = None;
                    self.finished = true;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

type StateSharing<F> = (
    SharingTask<F, StateFlow<<F as Flow>::Item>>,
    StateFlow<<F as Flow>::Item>,
);

type EventSharing<F> = (
    SharingTask<F, MutableSharedFlow<<F as Flow>::Item>>,
    SharedFlow<<F as Flow>::Item>,
);

pub(crate) fn state_sharing<F: Flow>(
    upstream: F,
    started: SharingStarted,
    initial: F::Item,
) -> StateSharing<F>
where
    F::Item: Clone,
{
    let shared = StateShared::new(initial.clone());
    let subscribers = shared.subscribers.clone();
    let state = StateFlow::from_shared(Arc::clone(&shared));
    let task = SharingTask::new(upstream, started, state.clone(), subscribers, Some(initial));
    (task, state)
}

pub(crate) fn shared_sharing<F: Flow>(
    upstream: F,
    started: SharingStarted,
    replay: usize,
) -> EventSharing<F>
where
    F::Item: Clone,
{
    let extra = replay.max(SHARE_IN_BUFFER) - replay;
    let events = MutableSharedFlow::with_overflow(replay, extra, BufferOverflow::Suspend);
    let subscribers = events.subscribers().clone();
    let view = events.as_shared_flow();
    let task = SharingTask::new(upstream, started, events, subscribers, None);
    (task, view)
}

impl<F: Flow> Future for SharingTask<F, StateFlow<F::Item>>
where
    F::Item: Clone + PartialEq,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.get_mut().poll_sharing(cx)
    }
}

impl<F: Flow> Future for SharingTask<F, MutableSharedFlow<F::Item>>
where
    F::Item: Clone,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.get_mut().poll_sharing(cx)
    }
}

impl<F: Flow, S> Drop for SharingTask<F, S> {
    fn drop(&mut self) {
        self.subscribers.watch().unsubscribe(self.counter_key);
    }
}

/// The coroutine behind [`state_in_first`](crate::FlowExt::state_in_first):
/// it waits for the upstream's first value, hands over a state holding it,
/// and keeps the state up to date.
pub struct FirstState<R: Stream> {
    run: R,
    sender: Option<OneshotSender<StateFlow<R::Item>>>,
    state: Option<StateFlow<R::Item>>,
}

impl<R: Stream> FirstState<R> {
    pub(crate) fn start(run: R) -> (Self, StateInFirst<R::Item>) {
        let (sender, receiver) = oneshot();
        let task = Self {
            run,
            sender: Some(sender),
            state: None,
        };
        (task, StateInFirst { receiver })
    }
}

impl<R: Stream> Unpin for FirstState<R> {}

impl<R> Future for FirstState<R>
where
    R: Stream + Unpin,
    R::Item: Clone + PartialEq,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(value)) => match &this.state {
                    Some(state) => state.publish(value),
                    None => {
                        let state = StateFlow::from_shared(StateShared::new(value));
                        if let Some(sender) = this.sender.take() {
                            sender.send(state.clone());
                        }
                        this.state = Some(state);
                    }
                },
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The future returned by [`state_in_first`](crate::FlowExt::state_in_first):
/// the state once the upstream emitted, or `None` if it completed without a
/// value.
pub struct StateInFirst<T> {
    receiver: OneshotReceiver<StateFlow<T>>,
}

impl<T> Unpin for StateInFirst<T> {}

impl<T> Future for StateInFirst<T> {
    type Output = Option<StateFlow<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<StateFlow<T>>> {
        Pin::new(&mut self.get_mut().receiver).poll(cx)
    }
}
