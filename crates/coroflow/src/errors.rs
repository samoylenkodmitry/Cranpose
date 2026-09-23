use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{clock::Timer, flow::Flow, operators::poll_run};

/// The flow returned by [`catch`](crate::FlowExt::catch).
#[derive(Clone)]
pub struct Catch<F, H> {
    upstream: F,
    handler: H,
}

impl<F, H> Catch<F, H> {
    pub(crate) fn new(upstream: F, handler: H) -> Self {
        Self { upstream, handler }
    }
}

/// One run of a [`Catch`].
pub struct CatchRun<S, H, G: Flow> {
    upstream: Option<S>,
    handler: H,
    fallback: Option<G::Run>,
}

impl<S, H, G: Flow> Unpin for CatchRun<S, H, G> {}

impl<F, H, G, T, E> Flow for Catch<F, H>
where
    F: Flow<Item = Result<T, E>>,
    H: Fn(E) -> G + Clone,
    G: Flow<Item = T>,
{
    type Item = T;
    type Run = CatchRun<F::Run, H, G>;

    fn open(&self) -> Self::Run {
        CatchRun {
            upstream: Some(self.upstream.open()),
            handler: self.handler.clone(),
            fallback: None,
        }
    }
}

impl<S, H, G, T, E> Stream for CatchRun<S, H, G>
where
    S: Stream<Item = Result<T, E>> + Unpin,
    H: Fn(E) -> G,
    G: Flow<Item = T>,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        if let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(Ok(value))) => return Poll::Ready(Some(value)),
                Poll::Ready(Some(Err(error))) => {
                    this.upstream = None;
                    this.fallback = Some((this.handler)(error).open());
                }
                Poll::Ready(None) => {
                    this.upstream = None;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        match this.fallback.as_mut() {
            Some(fallback) => poll_run(fallback, cx),
            None => Poll::Ready(None),
        }
    }
}

/// The flow returned by [`retry_when`](crate::FlowExt::retry_when).
#[derive(Clone)]
pub struct RetryWhen<F, P> {
    upstream: F,
    policy: P,
}

impl<F, P> RetryWhen<F, P> {
    pub(crate) fn new(upstream: F, policy: P) -> Self {
        Self { upstream, policy }
    }
}

/// One run of a [`RetryWhen`].
pub struct RetryRun<F: Flow, P> {
    upstream: F,
    run: Option<F::Run>,
    policy: P,
    attempt: u32,
    backoff: Timer,
    waiting: bool,
}

impl<F: Flow, P> Unpin for RetryRun<F, P> {}

impl<F, P, T, E> Flow for RetryWhen<F, P>
where
    F: Flow<Item = Result<T, E>> + Clone,
    P: Fn(&E, u32) -> Option<Duration> + Clone,
{
    type Item = Result<T, E>;
    type Run = RetryRun<F, P>;

    fn open(&self) -> Self::Run {
        RetryRun {
            run: Some(self.upstream.open()),
            upstream: self.upstream.clone(),
            policy: self.policy.clone(),
            attempt: 0,
            backoff: Timer::default(),
            waiting: false,
        }
    }
}

impl<F, P, T, E> Stream for RetryRun<F, P>
where
    F: Flow<Item = Result<T, E>>,
    P: Fn(&E, u32) -> Option<Duration>,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<T, E>>> {
        let this = self.get_mut();
        loop {
            if this.waiting {
                if this.backoff.poll_elapsed(cx).is_pending() {
                    return Poll::Pending;
                }
                this.waiting = false;
                this.run = Some(this.upstream.open());
            }
            let Some(run) = this.run.as_mut() else {
                return Poll::Ready(None);
            };
            match poll_run(run, cx) {
                Poll::Ready(Some(Err(error))) => match (this.policy)(&error, this.attempt) {
                    Some(backoff) => {
                        this.attempt += 1;
                        this.run = None;
                        this.waiting = true;
                        this.backoff.start(backoff);
                    }
                    None => {
                        this.run = None;
                        return Poll::Ready(Some(Err(error)));
                    }
                },
                other => return other,
            }
        }
    }
}
