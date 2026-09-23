use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// One of two results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Either<A, B> {
    /// The first future finished first.
    Left(A),
    /// The second future finished first.
    Right(B),
}

/// Waits for whichever of two futures finishes first and drops the other —
/// Kotlin's `select`, biased towards `first` when both are ready.
pub fn select<A: Future, B: Future>(first: A, second: B) -> Select<A, B> {
    Select {
        first: Box::pin(first),
        second: Box::pin(second),
    }
}

/// The future returned by [`select`].
pub struct Select<A, B> {
    first: Pin<Box<A>>,
    second: Pin<Box<B>>,
}

impl<A: Future, B: Future> Future for Select<A, B> {
    type Output = Either<A::Output, B::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Poll::Ready(value) = this.first.as_mut().poll(cx) {
            return Poll::Ready(Either::Left(value));
        }
        this.second.as_mut().poll(cx).map(Either::Right)
    }
}

/// Waits for the first of `futures` to finish and returns its index and
/// output, dropping the rest — biased towards earlier futures.
pub fn select_all<F: Future>(futures: Vec<F>) -> SelectAll<F> {
    SelectAll {
        futures: futures.into_iter().map(Box::pin).collect(),
    }
}

/// The future returned by [`select_all`].
pub struct SelectAll<F> {
    futures: Vec<Pin<Box<F>>>,
}

impl<F: Future> Future for SelectAll<F> {
    type Output = (usize, F::Output);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        for (index, future) in this.futures.iter_mut().enumerate() {
            if let Poll::Ready(value) = future.as_mut().poll(cx) {
                return Poll::Ready((index, value));
            }
        }
        Poll::Pending
    }
}

/// Lets other coroutines on the same dispatcher run before continuing —
/// Kotlin's `yield`.
pub fn yield_now() -> YieldNow {
    YieldNow { yielded: false }
}

/// The future returned by [`yield_now`].
pub struct YieldNow {
    yielded: bool,
}

impl Future for YieldNow {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if this.yielded {
            return Poll::Ready(());
        }
        this.yielded = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
