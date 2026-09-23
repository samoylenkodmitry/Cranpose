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
    state::{StateFlow, StateShared},
};

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

/// The coroutine behind [`state_in`](crate::FlowExt::state_in): it watches the
/// collector count and starts or stops the upstream.
pub struct SharingTask<F: Flow> {
    upstream: F,
    started: SharingStarted,
    shared: Arc<StateShared<F::Item>>,
    counter_key: usize,
    counter_seen: Option<u64>,
    subscribers: usize,
    opened_seen: u64,
    newly_subscribed: bool,
    ever_subscribed: bool,
    run: Option<F::Run>,
    finished: bool,
    stop_timer: Timer,
}

impl<F: Flow> Unpin for SharingTask<F> {}

impl<F: Flow> SharingTask<F>
where
    F::Item: Clone + PartialEq,
{
    pub(crate) fn new(
        upstream: F,
        started: SharingStarted,
        initial: F::Item,
    ) -> (Self, StateFlow<F::Item>) {
        let shared = StateShared::new(initial);
        let state = StateFlow::from_shared(Arc::clone(&shared));
        let counter_key = shared.subscribers.subscribe();
        let task = Self {
            upstream,
            started,
            shared,
            counter_key,
            counter_seen: None,
            subscribers: 0,
            opened_seen: 0,
            newly_subscribed: false,
            ever_subscribed: false,
            run: None,
            finished: false,
            stop_timer: Timer::default(),
        };
        (task, state)
    }

    fn observe_subscribers(&mut self, cx: &mut Context<'_>) {
        while let Poll::Ready(subscribers) = self.shared.subscribers.poll_changed(
            self.counter_key,
            &mut self.counter_seen,
            cx,
            |subscribers| *subscribers,
        ) {
            self.subscribers = subscribers.current;
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
                if self.subscribers > 0 {
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
}

impl<F: Flow> Future for SharingTask<F>
where
    F::Item: Clone + PartialEq,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        this.observe_subscribers(cx);
        if !this.wants_upstream(cx) {
            this.run = None;
            this.finished = false;
            this.stop_timer.cancel();
            return Poll::Pending;
        }
        if this.run.is_none() && !this.finished {
            this.run = Some(this.upstream.open());
        }
        while let Some(run) = this.run.as_mut() {
            match Pin::new(run).poll_next(cx) {
                Poll::Ready(Some(value)) => this.shared.set(value),
                Poll::Ready(None) => {
                    this.run = None;
                    this.finished = true;
                }
                Poll::Pending => break,
            }
        }
        Poll::Pending
    }
}

impl<F: Flow> Drop for SharingTask<F> {
    fn drop(&mut self) {
        self.shared.subscribers.unsubscribe(self.counter_key);
    }
}
