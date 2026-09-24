use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{
    builders::Emitter,
    flow::Flow,
    operators::poll_run,
    suspending::{Actions, Finish, Step},
};

/// The flow returned by [`with_index`](crate::FlowExt::with_index).
#[derive(Clone)]
pub struct WithIndex<F> {
    upstream: F,
}

impl<F> WithIndex<F> {
    pub(crate) fn new(upstream: F) -> Self {
        Self { upstream }
    }
}

/// One run of a [`WithIndex`].
pub struct WithIndexRun<S> {
    upstream: S,
    next: usize,
}

impl<S> Unpin for WithIndexRun<S> {}

impl<F: Flow> Flow for WithIndex<F> {
    type Item = (usize, F::Item);
    type Run = WithIndexRun<F::Run>;

    fn open(&self) -> Self::Run {
        WithIndexRun {
            upstream: self.upstream.open(),
            next: 0,
        }
    }
}

impl<S: Stream + Unpin> Stream for WithIndexRun<S> {
    type Item = (usize, S::Item);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        poll_run(&mut this.upstream, cx).map(|value| {
            value.map(|value| {
                let index = this.next;
                this.next += 1;
                (index, value)
            })
        })
    }
}

/// The flow returned by [`running_reduce`](crate::FlowExt::running_reduce).
#[derive(Clone)]
pub struct RunningReduce<F, R> {
    upstream: F,
    reducer: R,
}

impl<F, R> RunningReduce<F, R> {
    pub(crate) fn new(upstream: F, reducer: R) -> Self {
        Self { upstream, reducer }
    }
}

/// One run of a [`RunningReduce`].
pub struct RunningReduceRun<S: Stream, R> {
    upstream: S,
    reducer: R,
    accumulated: Option<S::Item>,
}

impl<S: Stream, R> Unpin for RunningReduceRun<S, R> {}

impl<F, R> Flow for RunningReduce<F, R>
where
    F: Flow,
    F::Item: Clone,
    R: Fn(&F::Item, F::Item) -> F::Item + Clone,
{
    type Item = F::Item;
    type Run = RunningReduceRun<F::Run, R>;

    fn open(&self) -> Self::Run {
        RunningReduceRun {
            upstream: self.upstream.open(),
            reducer: self.reducer.clone(),
            accumulated: None,
        }
    }
}

impl<S, R> Stream for RunningReduceRun<S, R>
where
    S: Stream + Unpin,
    S::Item: Clone,
    R: Fn(&S::Item, S::Item) -> S::Item,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        poll_run(&mut this.upstream, cx).map(|value| {
            value.map(|value| {
                let next = match this.accumulated.take() {
                    Some(accumulated) => (this.reducer)(&accumulated, value),
                    None => value,
                };
                this.accumulated = Some(next.clone());
                next
            })
        })
    }
}

/// The flow returned by [`chunked`](crate::FlowExt::chunked).
#[derive(Clone)]
pub struct Chunked<F> {
    upstream: F,
    size: usize,
}

impl<F> Chunked<F> {
    pub(crate) fn new(upstream: F, size: usize) -> Self {
        Self {
            upstream,
            size: size.max(1),
        }
    }
}

/// One run of a [`Chunked`].
pub struct ChunkedRun<S: Stream> {
    upstream: Option<S>,
    size: usize,
    chunk: Vec<S::Item>,
}

impl<S: Stream> Unpin for ChunkedRun<S> {}

impl<F: Flow> Flow for Chunked<F> {
    type Item = Vec<F::Item>;
    type Run = ChunkedRun<F::Run>;

    fn open(&self) -> Self::Run {
        ChunkedRun {
            upstream: Some(self.upstream.open()),
            size: self.size,
            chunk: Vec::new(),
        }
    }
}

impl<S: Stream + Unpin> Stream for ChunkedRun<S> {
    type Item = Vec<S::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => {
                    if this.chunk.capacity() == 0 {
                        this.chunk.reserve_exact(this.size);
                    }
                    this.chunk.push(value);
                    if this.chunk.len() == this.size {
                        return Poll::Ready(Some(std::mem::take(&mut this.chunk)));
                    }
                }
                Poll::Ready(None) => this.upstream = None,
                Poll::Pending => return Poll::Pending,
            }
        }
        if this.chunk.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Ready(Some(std::mem::take(&mut this.chunk)))
        }
    }
}

/// The step of [`on_empty`](crate::FlowExt::on_empty): its action gets only
/// an [`Emitter`].
pub struct Fallback<U>(PhantomData<fn() -> U>);

impl<U, F, Fut> Step<(), F> for Fallback<U>
where
    F: FnOnce(Emitter<U>) -> Fut,
    Fut: Future<Output = ()>,
{
    type Item = U;
    type Held = ();
    type Action = Fut;

    fn start(action: F, (): (), emitter: &mut Option<Emitter<U>>) -> (Fut, ()) {
        (action(emitter.get_or_insert_with(Emitter::new).clone()), ())
    }

    fn finish((): (), (): ()) -> Finish<U> {
        Finish::Skip
    }
}

/// The flow returned by [`on_empty`](crate::FlowExt::on_empty).
#[derive(Clone)]
pub struct OnEmpty<F, A> {
    upstream: F,
    action: A,
}

impl<F, A> OnEmpty<F, A> {
    pub(crate) fn new(upstream: F, action: A) -> Self {
        Self { upstream, action }
    }
}

/// One run of an [`OnEmpty`].
pub struct OnEmptyRun<S: Stream, A>
where
    Fallback<S::Item>: Step<(), A>,
{
    upstream: Option<S>,
    emitted: bool,
    fallback: Actions<(), A, Fallback<S::Item>>,
}

impl<S: Stream, A> Unpin for OnEmptyRun<S, A> where Fallback<S::Item>: Step<(), A> {}

impl<F, A, Fut> Flow for OnEmpty<F, A>
where
    F: Flow,
    A: FnOnce(Emitter<F::Item>) -> Fut + Clone,
    Fut: Future<Output = ()>,
{
    type Item = F::Item;
    type Run = OnEmptyRun<F::Run, A>;

    fn open(&self) -> Self::Run {
        OnEmptyRun {
            upstream: Some(self.upstream.open()),
            emitted: false,
            fallback: Actions::new(self.action.clone()),
        }
    }
}

impl<S, A, Fut> Stream for OnEmptyRun<S, A>
where
    S: Stream + Unpin,
    A: FnOnce(Emitter<S::Item>) -> Fut + Clone,
    Fut: Future<Output = ()>,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        if let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => {
                    this.emitted = true;
                    return Poll::Ready(Some(value));
                }
                Poll::Ready(None) => {
                    this.upstream = None;
                    if !this.emitted {
                        this.fallback.start(());
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        match this.fallback.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Finish::Emit(value))) => Poll::Ready(Some(value)),
            Poll::Ready(Some(Finish::Skip | Finish::Stop) | None) => Poll::Ready(None),
        }
    }
}
