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
