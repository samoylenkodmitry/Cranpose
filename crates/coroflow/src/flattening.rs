use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{combining::poll_fairly, flow::Flow, operators::poll_run};

#[derive(Clone, Copy)]
enum Switching {
    Latest,
    Concurrent(usize),
}

/// The flow returned by [`flatten_concat`](crate::FlowExt::flatten_concat)
/// and [`flatten_merge`](crate::FlowExt::flatten_merge).
pub type Flatten<F> = FlatMap<F, fn(<F as Flow>::Item) -> <F as Flow>::Item>;

/// The flow returned by [`flat_map_latest`](crate::FlowExt::flat_map_latest),
/// [`flat_map_merge`](crate::FlowExt::flat_map_merge) and
/// [`flat_map_concat`](crate::FlowExt::flat_map_concat).
#[derive(Clone)]
pub struct FlatMap<F, T> {
    upstream: F,
    transform: T,
    switching: Switching,
}

impl<F, T> FlatMap<F, T> {
    pub(crate) fn latest(upstream: F, transform: T) -> Self {
        Self {
            upstream,
            transform,
            switching: Switching::Latest,
        }
    }

    pub(crate) fn concurrent(upstream: F, transform: T, concurrency: usize) -> Self {
        Self {
            upstream,
            transform,
            switching: Switching::Concurrent(concurrency.max(1)),
        }
    }
}

/// One run of a [`FlatMap`].
pub struct FlatMapRun<S, T, G: Flow> {
    upstream: Option<S>,
    transform: T,
    inners: Vec<Option<G::Run>>,
    switching: Switching,
    next: usize,
}

impl<S, T, G: Flow> Unpin for FlatMapRun<S, T, G> {}

impl<F, T, G> Flow for FlatMap<F, T>
where
    F: Flow,
    G: Flow,
    T: Fn(F::Item) -> G + Clone,
{
    type Item = G::Item;
    type Run = FlatMapRun<F::Run, T, G>;

    fn open(&self) -> Self::Run {
        let capacity = match self.switching {
            Switching::Latest => 1,
            Switching::Concurrent(concurrency) => concurrency,
        };
        FlatMapRun {
            upstream: Some(self.upstream.open()),
            transform: self.transform.clone(),
            inners: Vec::with_capacity(capacity),
            switching: self.switching,
            next: 0,
        }
    }
}

impl<S, T, G> FlatMapRun<S, T, G>
where
    S: Stream + Unpin,
    G: Flow,
    T: Fn(S::Item) -> G,
{
    fn pull_upstream(&mut self, cx: &mut Context<'_>) {
        while let Some(upstream) = self.upstream.as_mut() {
            if let Switching::Concurrent(concurrency) = self.switching
                && self.inners.len() >= concurrency
            {
                return;
            }
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => {
                    if let Switching::Latest = self.switching {
                        self.inners.clear();
                    }
                    self.inners.push(Some((self.transform)(value).open()));
                }
                Poll::Ready(None) => self.upstream = None,
                Poll::Pending => return,
            }
        }
    }
}

impl<S, T, G> Stream for FlatMapRun<S, T, G>
where
    S: Stream + Unpin,
    G: Flow,
    T: Fn(S::Item) -> G,
{
    type Item = G::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<G::Item>> {
        let this = self.get_mut();
        loop {
            this.inners.retain(Option::is_some);
            this.pull_upstream(cx);
            if this.inners.is_empty() {
                return if this.upstream.is_none() {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                };
            }
            match poll_fairly(&mut this.inners, &mut this.next, cx) {
                Poll::Ready(Some(value)) => return Poll::Ready(Some(value)),
                Poll::Ready(None) => continue,
                Poll::Pending => {
                    let freed = this.inners.iter().any(Option::is_none);
                    if !(freed && this.upstream.is_some()) {
                        return Poll::Pending;
                    }
                }
            }
        }
    }
}
