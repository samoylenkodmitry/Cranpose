use std::{
    marker::PhantomData,
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

/// Emits values while the predicate holds, then completes — the
/// [`take_while`](crate::FlowExt::take_while) kind of [`While`].
pub struct Taking;

/// Ignores values while the predicate holds, then emits everything — the
/// [`skip_while`](crate::FlowExt::skip_while) kind of [`While`].
pub struct Skipping;

/// The flow returned by [`take_while`](crate::FlowExt::take_while) and
/// [`skip_while`](crate::FlowExt::skip_while).
pub struct While<F, P, K> {
    upstream: F,
    predicate: P,
    _kind: PhantomData<fn() -> K>,
}

impl<F, P, K> While<F, P, K> {
    pub(crate) fn new(upstream: F, predicate: P) -> Self {
        Self {
            upstream,
            predicate,
            _kind: PhantomData,
        }
    }
}

impl<F: Clone, P: Clone, K> Clone for While<F, P, K> {
    fn clone(&self) -> Self {
        Self::new(self.upstream.clone(), self.predicate.clone())
    }
}

/// One run of a [`While`].
pub struct WhileRun<S, P, K> {
    upstream: Option<S>,
    predicate: P,
    deciding: bool,
    _kind: PhantomData<fn() -> K>,
}

impl<S, P, K> Unpin for WhileRun<S, P, K> {}

impl<F, P, K> Flow for While<F, P, K>
where
    F: Flow,
    P: Fn(&F::Item) -> bool + Clone,
    WhileRun<F::Run, P, K>: Stream<Item = F::Item>,
{
    type Item = F::Item;
    type Run = WhileRun<F::Run, P, K>;

    fn open(&self) -> Self::Run {
        WhileRun {
            upstream: Some(self.upstream.open()),
            predicate: self.predicate.clone(),
            deciding: true,
            _kind: PhantomData,
        }
    }
}

impl<S, P> Stream for WhileRun<S, P, Taking>
where
    S: Stream + Unpin,
    P: Fn(&S::Item) -> bool,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        let Some(upstream) = this.upstream.as_mut() else {
            return Poll::Ready(None);
        };
        match poll_run(upstream, cx) {
            Poll::Ready(Some(value)) if (this.predicate)(&value) => Poll::Ready(Some(value)),
            Poll::Ready(_) => {
                this.upstream = None;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S, P> Stream for WhileRun<S, P, Skipping>
where
    S: Stream + Unpin,
    P: Fn(&S::Item) -> bool,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        let Some(upstream) = this.upstream.as_mut() else {
            return Poll::Ready(None);
        };
        loop {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) if this.deciding && (this.predicate)(&value) => {}
                Poll::Ready(Some(value)) => {
                    this.deciding = false;
                    return Poll::Ready(Some(value));
                }
                other => return other,
            }
        }
    }
}
