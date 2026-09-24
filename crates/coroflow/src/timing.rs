use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    clock::{TimedOut, Timer},
    flow::Flow,
    operators::poll_run,
};

/// The flow returned by [`sample`](crate::FlowExt::sample).
#[derive(Clone)]
pub struct Sample<F> {
    upstream: F,
    period: Duration,
}

impl<F> Sample<F> {
    pub(crate) fn new(upstream: F, period: Duration) -> Self {
        Self { upstream, period }
    }
}

/// One run of a [`Sample`].
pub struct SampleRun<S: Stream> {
    upstream: Option<S>,
    latest: Option<S::Item>,
    period: Duration,
    ticker: Timer,
}

impl<S: Stream> Unpin for SampleRun<S> {}

impl<F: Flow> Flow for Sample<F> {
    type Item = F::Item;
    type Run = SampleRun<F::Run>;

    fn open(&self) -> Self::Run {
        SampleRun {
            upstream: Some(self.upstream.open()),
            latest: None,
            period: self.period,
            ticker: Timer::default(),
        }
    }
}

impl<S: Stream + Unpin> Stream for SampleRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        if !this.ticker.is_armed() {
            this.ticker.start(this.period);
        }
        while let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => this.latest = Some(value),
                Poll::Ready(None) => {
                    this.upstream = None;
                    this.ticker.cancel();
                    return Poll::Ready(None);
                }
                Poll::Pending => break,
            }
        }
        if this.upstream.is_none() {
            return Poll::Ready(None);
        }
        while this.ticker.poll_elapsed(cx).is_ready() {
            this.ticker.start(this.period);
            if let Some(value) = this.latest.take() {
                return Poll::Ready(Some(value));
            }
        }
        Poll::Pending
    }
}

/// The flow returned by [`timeout`](crate::FlowExt::timeout).
#[derive(Clone)]
pub struct Timeout<F> {
    upstream: F,
    limit: Duration,
}

impl<F> Timeout<F> {
    pub(crate) fn new(upstream: F, limit: Duration) -> Self {
        Self { upstream, limit }
    }
}

/// One run of a [`Timeout`].
pub struct TimeoutRun<S> {
    upstream: Option<S>,
    limit: Duration,
    timer: Timer,
}

impl<S> Unpin for TimeoutRun<S> {}

impl<F: Flow> Flow for Timeout<F> {
    type Item = Result<F::Item, TimedOut>;
    type Run = TimeoutRun<F::Run>;

    fn open(&self) -> Self::Run {
        TimeoutRun {
            upstream: Some(self.upstream.open()),
            limit: self.limit,
            timer: Timer::default(),
        }
    }
}

impl<S: Stream + Unpin> Stream for TimeoutRun<S> {
    type Item = Result<S::Item, TimedOut>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let Some(upstream) = this.upstream.as_mut() else {
            return Poll::Ready(None);
        };
        if !this.timer.is_armed() {
            this.timer.start(this.limit);
        }
        match poll_run(upstream, cx) {
            Poll::Ready(Some(value)) => {
                this.timer.start(this.limit);
                return Poll::Ready(Some(Ok(value)));
            }
            Poll::Ready(None) => {
                this.upstream = None;
                this.timer.cancel();
                return Poll::Ready(None);
            }
            Poll::Pending => {}
        }
        this.timer.poll_elapsed(cx).map(|()| {
            this.upstream = None;
            Some(Err(TimedOut))
        })
    }
}
