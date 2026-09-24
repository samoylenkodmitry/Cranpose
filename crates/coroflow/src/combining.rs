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

macro_rules! combine_n {
    ($function:ident, $name:ident, $run:ident, $count:literal, $($flow:ident $field:ident),+) => {
        #[doc = concat!(
            "Combines the latest values of ", $count,
            " flows once each has emitted — Kotlin's `combine`."
        )]
        pub fn $function<$($flow,)+ F, U>($($field: $flow,)+ combiner: F) -> $name<$($flow,)+ F>
        where
            $($flow: Flow,)+
            F: Fn($(&$flow::Item),+) -> U + Clone,
        {
            $name {
                $($field,)+
                combiner,
            }
        }

        #[doc = concat!("The flow returned by [`", stringify!($function), "`].")]
        #[derive(Clone)]
        pub struct $name<$($flow,)+ F> {
            $($field: $flow,)+
            combiner: F,
        }

        #[doc = concat!("One run of a [`", stringify!($name), "`].")]
        pub struct $run<$($flow: Stream,)+ F> {
            $($field: Latest<$flow>,)+
            combiner: F,
        }

        impl<$($flow: Stream,)+ F> Unpin for $run<$($flow,)+ F> {}

        impl<$($flow,)+ F, U> Flow for $name<$($flow,)+ F>
        where
            $($flow: Flow,)+
            F: Fn($(&$flow::Item),+) -> U + Clone,
        {
            type Item = U;
            type Run = $run<$($flow::Run,)+ F>;

            fn open(&self) -> Self::Run {
                $run {
                    $($field: Latest::new(self.$field.open()),)+
                    combiner: self.combiner.clone(),
                }
            }
        }

        impl<$($flow,)+ F, U> Stream for $run<$($flow,)+ F>
        where
            $($flow: Stream + Unpin,)+
            F: Fn($(&$flow::Item),+) -> U,
        {
            type Item = U;

            fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
                let this = self.get_mut();
                let changed = [$(this.$field.drain(cx)),+].contains(&true);
                if changed && let ($(Some($field),)+) = ($(this.$field.value(),)+) {
                    return Poll::Ready(Some((this.combiner)($($field),+)));
                }
                latest_ended(&[$(this.$field.status()),+])
            }
        }
    };
}

combine_n!(combine3, Combine3, Combine3Run, "three", A first, B second, C third);
combine_n!(combine4, Combine4, Combine4Run, "four", A first, B second, C third, D fourth);
combine_n!(
    combine5,
    Combine5,
    Combine5Run,
    "five",
    A first,
    B second,
    C third,
    D fourth,
    E fifth
);

/// Combines the latest values of every flow in `flows` once each has
/// emitted, handing them to `combiner` in order — Kotlin's
/// `combine(flows) { values -> }`.
pub fn combine_all<F, C, U>(flows: Vec<F>, combiner: C) -> CombineAll<F, C>
where
    F: Flow,
    C: Fn(&[F::Item]) -> U + Clone,
{
    CombineAll { flows, combiner }
}

/// The flow returned by [`combine_all`].
#[derive(Clone)]
pub struct CombineAll<F, C> {
    flows: Vec<F>,
    combiner: C,
}

/// One run of a [`CombineAll`].
pub struct CombineAllRun<S: Stream, C> {
    runs: Vec<Option<S>>,
    first_values: Vec<Option<S::Item>>,
    values: Vec<S::Item>,
    combiner: C,
}

impl<S: Stream, C> Unpin for CombineAllRun<S, C> {}

impl<F, C, U> Flow for CombineAll<F, C>
where
    F: Flow,
    C: Fn(&[F::Item]) -> U + Clone,
{
    type Item = U;
    type Run = CombineAllRun<F::Run, C>;

    fn open(&self) -> Self::Run {
        CombineAllRun {
            runs: self.flows.iter().map(|flow| Some(flow.open())).collect(),
            first_values: self.flows.iter().map(|_| None).collect(),
            values: Vec::new(),
            combiner: self.combiner.clone(),
        }
    }
}

impl<S, C, U> Stream for CombineAllRun<S, C>
where
    S: Stream + Unpin,
    C: Fn(&[S::Item]) -> U,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        let mut changed = false;
        for (index, slot) in this.runs.iter_mut().enumerate() {
            while let Some(run) = slot.as_mut() {
                match poll_run(run, cx) {
                    Poll::Ready(Some(value)) => {
                        changed = true;
                        if let Some(current) = this.values.get_mut(index) {
                            *current = value;
                        } else if let Some(first) = this.first_values.get_mut(index) {
                            *first = Some(value);
                        }
                    }
                    Poll::Ready(None) => *slot = None,
                    Poll::Pending => break,
                }
            }
        }
        if this.values.is_empty()
            && !this.first_values.is_empty()
            && this.first_values.iter().all(Option::is_some)
        {
            this.values = this.first_values.drain(..).flatten().collect();
        }
        if changed && !this.values.is_empty() {
            return Poll::Ready(Some((this.combiner)(&this.values)));
        }
        let starved = this
            .runs
            .iter()
            .zip(&this.first_values)
            .any(|(run, first)| run.is_none() && first.is_none());
        if this.runs.iter().all(Option::is_none) || starved {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}
