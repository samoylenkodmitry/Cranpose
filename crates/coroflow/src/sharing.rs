use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    clock::Timer,
    flow::Flow,
    state::{MutableSharedFlow, SharedFlow, StateFlow, StateShared, SubscriberCount},
};

/// How many values [`share_in`](crate::FlowExt::share_in) buffers beyond its
/// replay for collectors that fall behind — Kotlin's default channel size.
pub const SHARE_IN_BUFFER: usize = 64;

/// When a shared flow runs its upstream — Kotlin's `SharingStarted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    },
}

impl SharingStarted {
    /// [`SharingStarted::WhileSubscribed`] with `stop_timeout`.
    pub const fn while_subscribed(stop_timeout: Duration) -> Self {
        Self::WhileSubscribed { stop_timeout }
    }
}

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
    run: Option<F::Run>,
    finished: bool,
    stop_timer: Timer,
}

impl<F: Flow, S> Unpin for SharingTask<F, S> {}

impl<F: Flow, S> SharingTask<F, S> {
    fn new(upstream: F, started: SharingStarted, target: S, subscribers: SubscriberCount) -> Self {
        let counter_key = subscribers.watch().subscribe();
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
            run: None,
            finished: false,
            stop_timer: Timer::default(),
        }
    }

    fn observe_subscribers(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(subscribers) = self.subscribers.watch().poll_changed(
            self.counter_key,
            &mut self.counter_seen,
            cx,
            |subscribers| *subscribers,
        ) {
            self.current = subscribers.current;
            if subscribers.opened > self.opened_seen {
                self.opened_seen = subscribers.opened;
                self.newly_subscribed = true;
            }
        }
    }

    fn wants_upstream(&mut self, cx: &mut Context<'_>) -> bool {
        let newly_subscribed = std::mem::take(&mut self.newly_subscribed);
        match self.started {
            SharingStarted::Eagerly => true,
            SharingStarted::Lazily => {
                self.ever_subscribed |= newly_subscribed;
                self.ever_subscribed
            }
            SharingStarted::WhileSubscribed { stop_timeout } => {
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

    fn poll_sharing(&mut self, cx: &mut Context<'_>, publish: impl Fn(&S, F::Item)) -> Poll<()> {
        self.observe_subscribers(cx);
        if !self.wants_upstream(cx) {
            self.run = None;
            self.finished = false;
            self.stop_timer.cancel();
            return Poll::Pending;
        }
        if self.run.is_none() && !self.finished {
            self.run = Some(self.upstream.open());
        }
        while let Some(run) = self.run.as_mut() {
            match Pin::new(run).poll_next(cx) {
                Poll::Ready(Some(value)) => publish(&self.target, value),
                Poll::Ready(None) => {
                    self.run = None;
                    self.finished = true;
                }
                Poll::Pending => break,
            }
        }
        Poll::Pending
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
) -> StateSharing<F> {
    let shared = StateShared::new(initial);
    let subscribers = shared.subscribers.clone();
    let state = StateFlow::from_shared(Arc::clone(&shared));
    let task = SharingTask::new(upstream, started, state.clone(), subscribers);
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
    let events = MutableSharedFlow::new(replay, SHARE_IN_BUFFER);
    let subscribers = events.subscribers().clone();
    let view = events.as_shared_flow();
    let task = SharingTask::new(upstream, started, events, subscribers);
    (task, view)
}

impl<F: Flow> Future for SharingTask<F, StateFlow<F::Item>>
where
    F::Item: Clone + PartialEq,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.get_mut()
            .poll_sharing(cx, |state, value| state.publish(value))
    }
}

impl<F: Flow> Future for SharingTask<F, MutableSharedFlow<F::Item>>
where
    F::Item: Clone,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.get_mut()
            .poll_sharing(cx, |events, value| events.emit(value))
    }
}

impl<F: Flow, S> Drop for SharingTask<F, S> {
    fn drop(&mut self) {
        self.subscribers.watch().unsubscribe(self.counter_key);
    }
}
