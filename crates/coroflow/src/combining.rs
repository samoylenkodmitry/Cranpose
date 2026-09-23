use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{
    flow::Flow,
    operators::{Latest, latest_ended, poll_run},
};

/// Emits the values of all `flows` as they arrive, completing once every one
/// has completed — Kotlin's `merge`.
///
/// Flows of different types merge after [`boxed`](crate::FlowExt::boxed).
pub fn merge<F: Flow>(flows: Vec<F>) -> Merge<F> {
    Merge { flows }
}

/// The flow returned by [`merge`].
#[derive(Clone)]
pub struct Merge<F> {
    flows: Vec<F>,
}

/// One run of a [`Merge`].
pub struct MergeRun<S> {
    runs: Vec<Option<S>>,
    next: usize,
}

impl<F: Flow> Flow for Merge<F> {
    type Item = F::Item;
    type Run = MergeRun<F::Run>;

    fn open(&self) -> Self::Run {
        MergeRun {
            runs: self.flows.iter().map(|flow| Some(flow.open())).collect(),
            next: 0,
        }
    }
}

impl<S> Unpin for MergeRun<S> {}

pub(crate) fn poll_fairly<S: Stream + Unpin>(
    runs: &mut [Option<S>],
    next: &mut usize,
    cx: &mut Context<'_>,
) -> Poll<Option<S::Item>> {
    let count = runs.len();
    let mut live = false;
    for offset in 0..count {
        let index = (*next + offset) % count;
        let Some(run) = runs[index].as_mut() else {
            continue;
        };
        match poll_run(run, cx) {
            Poll::Ready(Some(value)) => {
                *next = (index + 1) % count;
                return Poll::Ready(Some(value));
            }
            Poll::Ready(None) => runs[index] = None,
            Poll::Pending => live = true,
        }
    }
    if live {
        Poll::Pending
    } else {
        Poll::Ready(None)
    }
}

impl<S: Stream + Unpin> Stream for MergeRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        poll_fairly(&mut this.runs, &mut this.next, cx)
    }
}

/// The flow returned by [`zip`](crate::FlowExt::zip).
#[derive(Clone)]
pub struct Zip<A, B, F> {
    first: A,
    second: B,
    combiner: F,
}

impl<A, B, F> Zip<A, B, F> {
    pub(crate) fn new(first: A, second: B, combiner: F) -> Self {
        Self {
            first,
            second,
            combiner,
        }
    }
}

/// One run of a [`Zip`].
pub struct ZipRun<A: Stream, B: Stream, F> {
    first: A,
    second: B,
    first_value: Option<A::Item>,
    second_value: Option<B::Item>,
    combiner: F,
    done: bool,
}

impl<A: Stream, B: Stream, F> Unpin for ZipRun<A, B, F> {}

impl<A, B, F, U> Flow for Zip<A, B, F>
where
    A: Flow,
    B: Flow,
    F: Fn(A::Item, B::Item) -> U + Clone,
{
    type Item = U;
    type Run = ZipRun<A::Run, B::Run, F>;

    fn open(&self) -> Self::Run {
        ZipRun {
            first: self.first.open(),
            second: self.second.open(),
            first_value: None,
            second_value: None,
            combiner: self.combiner.clone(),
            done: false,
        }
    }
}

fn fill<S: Stream + Unpin>(
    run: &mut S,
    slot: &mut Option<S::Item>,
    done: &mut bool,
    cx: &mut Context<'_>,
) {
    if slot.is_some() || *done {
        return;
    }
    match poll_run(run, cx) {
        Poll::Ready(Some(value)) => *slot = Some(value),
        Poll::Ready(None) => *done = true,
        Poll::Pending => {}
    }
}

impl<A, B, F, U> Stream for ZipRun<A, B, F>
where
    A: Stream + Unpin,
    B: Stream + Unpin,
    F: Fn(A::Item, B::Item) -> U,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        fill(&mut this.first, &mut this.first_value, &mut this.done, cx);
        fill(&mut this.second, &mut this.second_value, &mut this.done, cx);
        if let (Some(first), Some(second)) = (this.first_value.take(), this.second_value.take()) {
            return Poll::Ready(Some((this.combiner)(first, second)));
        }
        if this.done {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// Combines the latest values of three flows once each has emitted —
/// Kotlin's `combine(a, b, c)`.
pub fn combine3<A, B, C, F, U>(first: A, second: B, third: C, combiner: F) -> Combine3<A, B, C, F>
where
    A: Flow,
    B: Flow,
    C: Flow,
    F: Fn(&A::Item, &B::Item, &C::Item) -> U + Clone,
{
    Combine3 {
        first,
        second,
        third,
        combiner,
    }
}

/// The flow returned by [`combine3`].
#[derive(Clone)]
pub struct Combine3<A, B, C, F> {
    first: A,
    second: B,
    third: C,
    combiner: F,
}

/// One run of a [`Combine3`].
pub struct Combine3Run<A: Stream, B: Stream, C: Stream, F> {
    first: Latest<A>,
    second: Latest<B>,
    third: Latest<C>,
    combiner: F,
}

impl<A: Stream, B: Stream, C: Stream, F> Unpin for Combine3Run<A, B, C, F> {}

impl<A, B, C, F, U> Flow for Combine3<A, B, C, F>
where
    A: Flow,
    B: Flow,
    C: Flow,
    F: Fn(&A::Item, &B::Item, &C::Item) -> U + Clone,
{
    type Item = U;
    type Run = Combine3Run<A::Run, B::Run, C::Run, F>;

    fn open(&self) -> Self::Run {
        Combine3Run {
            first: Latest::new(self.first.open()),
            second: Latest::new(self.second.open()),
            third: Latest::new(self.third.open()),
            combiner: self.combiner.clone(),
        }
    }
}

impl<A, B, C, F, U> Stream for Combine3Run<A, B, C, F>
where
    A: Stream + Unpin,
    B: Stream + Unpin,
    C: Stream + Unpin,
    F: Fn(&A::Item, &B::Item, &C::Item) -> U,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        let changed = this.first.drain(cx) | this.second.drain(cx) | this.third.drain(cx);
        if changed
            && let (Some(first), Some(second), Some(third)) =
                (this.first.value(), this.second.value(), this.third.value())
        {
            return Poll::Ready(Some((this.combiner)(first, second, third)));
        }
        latest_ended(&[
            this.first.status(),
            this.second.status(),
            this.third.status(),
        ])
    }
}
