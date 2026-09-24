use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;

/// The future returned by [`collect`](crate::FlowExt::collect).
pub struct Collect<S, F> {
    run: S,
    action: F,
}

impl<S, F> Collect<S, F> {
    pub(crate) fn new(run: S, action: F) -> Self {
        Self { run, action }
    }
}

impl<S, F> Unpin for Collect<S, F> {}

impl<S, F> Future for Collect<S, F>
where
    S: Stream + Unpin,
    F: FnMut(S::Item),
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(value)) => (this.action)(value),
                Poll::Ready(None) => return Poll::Ready(()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The future returned by [`first`](crate::FlowExt::first).
pub struct First<S> {
    run: S,
}

impl<S> First<S> {
    pub(crate) fn new(run: S) -> Self {
        Self { run }
    }
}

impl<S: Stream + Unpin> Future for First<S> {
    type Output = Option<S::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        Pin::new(&mut self.run).poll_next(cx)
    }
}

/// The future returned by [`to_vec`](crate::FlowExt::to_vec).
pub struct ToVec<S: Stream> {
    run: S,
    items: Vec<S::Item>,
}

impl<S: Stream> ToVec<S> {
    pub(crate) fn new(run: S) -> Self {
        Self {
            run,
            items: Vec::new(),
        }
    }
}

impl<S: Stream> Unpin for ToVec<S> {}

impl<S: Stream + Unpin> Future for ToVec<S> {
    type Output = Vec<S::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Vec<S::Item>> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(value)) => this.items.push(value),
                Poll::Ready(None) => return Poll::Ready(std::mem::take(&mut this.items)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The future returned by [`fold`](crate::FlowExt::fold),
/// [`count`](crate::FlowExt::count) and [`last`](crate::FlowExt::last).
pub struct Fold<S, A, F> {
    run: S,
    accumulated: Option<A>,
    step: F,
}

impl<S, A, F> Fold<S, A, F> {
    pub(crate) fn new(run: S, initial: A, step: F) -> Self {
        Self {
            run,
            accumulated: Some(initial),
            step,
        }
    }
}

impl<S, A, F> Unpin for Fold<S, A, F> {}

impl<S, A, F> Future for Fold<S, A, F>
where
    S: Stream + Unpin,
    F: FnMut(A, S::Item) -> A,
{
    type Output = A;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<A> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(value)) => {
                    if let Some(accumulated) = this.accumulated.take() {
                        this.accumulated = Some((this.step)(accumulated, value));
                    }
                }
                Poll::Ready(None) => {
                    if let Some(accumulated) = this.accumulated.take() {
                        return Poll::Ready(accumulated);
                    }
                    return Poll::Pending;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The future returned by [`count`](crate::FlowExt::count).
pub type Count<S, T> = Fold<S, usize, fn(usize, T) -> usize>;

/// The future returned by [`last`](crate::FlowExt::last).
pub type Last<S, T> = Fold<S, Option<T>, fn(Option<T>, T) -> Option<T>>;

/// The future returned by [`reduce`](crate::FlowExt::reduce).
pub struct Reduce<S: Stream, F> {
    run: S,
    accumulated: Option<S::Item>,
    reducer: F,
}

impl<S: Stream, F> Reduce<S, F> {
    pub(crate) fn new(run: S, reducer: F) -> Self {
        Self {
            run,
            accumulated: None,
            reducer,
        }
    }
}

impl<S: Stream, F> Unpin for Reduce<S, F> {}

impl<S, F> Future for Reduce<S, F>
where
    S: Stream + Unpin,
    F: FnMut(S::Item, S::Item) -> S::Item,
{
    type Output = Option<S::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(value)) => {
                    this.accumulated = Some(match this.accumulated.take() {
                        Some(accumulated) => (this.reducer)(accumulated, value),
                        None => value,
                    });
                }
                Poll::Ready(None) => return Poll::Ready(this.accumulated.take()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The future returned by [`single`](crate::FlowExt::single).
pub struct Single<S: Stream> {
    run: S,
    only: Option<S::Item>,
}

impl<S: Stream> Single<S> {
    pub(crate) fn new(run: S) -> Self {
        Self { run, only: None }
    }
}

impl<S: Stream> Unpin for Single<S> {}

impl<S: Stream + Unpin> Future for Single<S> {
    type Output = Option<S::Item>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match Pin::new(&mut this.run).poll_next(cx) {
                Poll::Ready(Some(_)) if this.only.is_some() => return Poll::Ready(None),
                Poll::Ready(Some(value)) => this.only = Some(value),
                Poll::Ready(None) => return Poll::Ready(this.only.take()),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
