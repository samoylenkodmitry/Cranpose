use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{flow::Flow, operators::poll_run};

/// The flow returned by [`scan`](crate::FlowExt::scan).
#[derive(Clone)]
pub struct Scan<F, A, S> {
    upstream: F,
    initial: A,
    step: S,
}

impl<F, A, S> Scan<F, A, S> {
    pub(crate) fn new(upstream: F, initial: A, step: S) -> Self {
        Self {
            upstream,
            initial,
            step,
        }
    }
}

/// One run of a [`Scan`].
pub struct ScanRun<R, A, S> {
    upstream: R,
    accumulator: A,
    announced: bool,
    step: S,
}

impl<R, A, S> Unpin for ScanRun<R, A, S> {}

impl<F, A, S> Flow for Scan<F, A, S>
where
    F: Flow,
    A: Clone,
    S: Fn(&A, F::Item) -> A + Clone,
{
    type Item = A;
    type Run = ScanRun<F::Run, A, S>;

    fn open(&self) -> Self::Run {
        ScanRun {
            upstream: self.upstream.open(),
            accumulator: self.initial.clone(),
            announced: false,
            step: self.step.clone(),
        }
    }
}

impl<R, A, S> Stream for ScanRun<R, A, S>
where
    R: Stream + Unpin,
    A: Clone,
    S: Fn(&A, R::Item) -> A,
{
    type Item = A;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<A>> {
        let this = self.get_mut();
        if !this.announced {
            this.announced = true;
            return Poll::Ready(Some(this.accumulator.clone()));
        }
        poll_run(&mut this.upstream, cx).map(|value| {
            value.map(|value| {
                this.accumulator = (this.step)(&this.accumulator, value);
                this.accumulator.clone()
            })
        })
    }
}

/// The flow returned by [`skip`](crate::FlowExt::skip).
#[derive(Clone)]
pub struct Skip<F> {
    upstream: F,
    count: usize,
}

impl<F> Skip<F> {
    pub(crate) fn new(upstream: F, count: usize) -> Self {
        Self { upstream, count }
    }
}

/// One run of a [`Skip`].
pub struct SkipRun<S> {
    upstream: S,
    remaining: usize,
}

impl<F: Flow> Flow for Skip<F> {
    type Item = F::Item;
    type Run = SkipRun<F::Run>;

    fn open(&self) -> Self::Run {
        SkipRun {
            upstream: self.upstream.open(),
            remaining: self.count,
        }
    }
}

impl<S: Stream + Unpin> Stream for SkipRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match poll_run(&mut this.upstream, cx) {
                Poll::Ready(Some(_)) if this.remaining > 0 => this.remaining -= 1,
                other => return other,
            }
        }
    }
}

/// The flow returned by [`filter_map`](crate::FlowExt::filter_map).
#[derive(Clone)]
pub struct FilterMap<F, T> {
    upstream: F,
    transform: T,
}

impl<F, T> FilterMap<F, T> {
    pub(crate) fn new(upstream: F, transform: T) -> Self {
        Self {
            upstream,
            transform,
        }
    }
}

/// One run of a [`FilterMap`].
pub struct FilterMapRun<S, T> {
    upstream: S,
    transform: T,
}

impl<S, T> Unpin for FilterMapRun<S, T> {}

impl<F, T, U> Flow for FilterMap<F, T>
where
    F: Flow,
    T: Fn(F::Item) -> Option<U> + Clone,
{
    type Item = U;
    type Run = FilterMapRun<F::Run, T>;

    fn open(&self) -> Self::Run {
        FilterMapRun {
            upstream: self.upstream.open(),
            transform: self.transform.clone(),
        }
    }
}

impl<S, T, U> Stream for FilterMapRun<S, T>
where
    S: Stream + Unpin,
    T: Fn(S::Item) -> Option<U>,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        loop {
            match poll_run(&mut this.upstream, cx) {
                Poll::Ready(Some(value)) => {
                    if let Some(mapped) = (this.transform)(value) {
                        return Poll::Ready(Some(mapped));
                    }
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
