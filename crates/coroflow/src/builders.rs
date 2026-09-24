use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures_core::Stream;

use crate::{flow::Flow, sync::lock};

/// Builds a cold flow from an async block — Kotlin's `flow { emit(x) }`.
///
/// `block` runs again for every collection. Each run allocates once; emitting
/// allocates nothing.
///
/// ```
/// use coroflow::{FlowExt, flow};
/// let numbers = flow(|emitter| async move {
///     for value in 1..=3 {
///         emitter.emit(value).await;
///     }
/// });
/// assert_eq!(pollster::block_on(numbers.to_vec()), vec![1, 2, 3]);
/// ```
pub fn flow<T, F, Fut>(block: F) -> FlowBlock<T, F>
where
    F: Fn(Emitter<T>) -> Fut,
    Fut: Future<Output = ()>,
{
    FlowBlock {
        block,
        _item: PhantomData,
    }
}

/// The flow returned by [`flow`].
pub struct FlowBlock<T, F> {
    block: F,
    _item: PhantomData<fn() -> T>,
}

impl<T, F: Clone> Clone for FlowBlock<T, F> {
    fn clone(&self) -> Self {
        Self {
            block: self.block.clone(),
            _item: PhantomData,
        }
    }
}

/// The handle a [`flow`] block emits values through — Kotlin's
/// `FlowCollector`.
pub struct Emitter<T> {
    slot: Arc<Mutex<Option<T>>>,
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self {
            slot: Arc::clone(&self.slot),
        }
    }
}

impl<T> Emitter<T> {
    pub(crate) fn new() -> Self {
        Self {
            slot: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn take_emitted(&self) -> Option<T> {
        lock(&self.slot).take()
    }

    /// Emits every value of `flow` in turn — Kotlin's `emitAll`.
    pub async fn emit_all<F: Flow<Item = T>>(&self, flow: F) {
        let mut run = flow.open();
        while let Some(value) = std::future::poll_fn(|cx| Pin::new(&mut run).poll_next(cx)).await {
            self.emit(value).await;
        }
    }

    /// Hands `value` to the collector and resumes once it was taken.
    pub fn emit(&self, value: T) -> Emit<'_, T> {
        Emit {
            emitter: self,
            value: Some(value),
        }
    }
}

/// The future returned by [`Emitter::emit`].
pub struct Emit<'a, T> {
    emitter: &'a Emitter<T>,
    value: Option<T>,
}

impl<T> Unpin for Emit<'_, T> {}

impl<T> Future for Emit<'_, T> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let mut slot = lock(&this.emitter.slot);
        if let Some(value) = this.value.take() {
            *slot = Some(value);
            return Poll::Pending;
        }
        if slot.is_some() {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

/// One run of a [`FlowBlock`].
pub struct FlowBlockRun<T, Fut> {
    future: Option<Pin<Box<Fut>>>,
    slot: Arc<Mutex<Option<T>>>,
}

impl<T, F, Fut> Flow for FlowBlock<T, F>
where
    F: Fn(Emitter<T>) -> Fut,
    Fut: Future<Output = ()>,
{
    type Item = T;
    type Run = FlowBlockRun<T, Fut>;

    fn open(&self) -> Self::Run {
        let slot = Arc::new(Mutex::new(None));
        let emitter = Emitter {
            slot: Arc::clone(&slot),
        };
        FlowBlockRun {
            future: Some(Box::pin((self.block)(emitter))),
            slot,
        }
    }
}

impl<T, Fut: Future<Output = ()>> Stream for FlowBlockRun<T, Fut> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(None);
        };
        let polled = future.as_mut().poll(cx);
        if polled.is_ready() {
            this.future = None;
        }
        match lock(&this.slot).take() {
            Some(value) => Poll::Ready(Some(value)),
            None if polled.is_ready() => Poll::Ready(None),
            None => Poll::Pending,
        }
    }
}

impl<T, Fut> Unpin for FlowBlockRun<T, Fut> {}

/// A cold flow that emits clones of `items` on every collection — Kotlin's
/// `flowOf`.
pub fn flow_of<T: Clone>(items: Vec<T>) -> FlowOf<T> {
    FlowOf { items }
}

/// The flow returned by [`flow_of`].
#[derive(Clone)]
pub struct FlowOf<T> {
    items: Vec<T>,
}

/// One run of a [`FlowOf`].
pub struct FlowOfRun<T> {
    items: std::vec::IntoIter<T>,
}

impl<T: Clone> Flow for FlowOf<T> {
    type Item = T;
    type Run = FlowOfRun<T>;

    fn open(&self) -> Self::Run {
        FlowOfRun {
            items: self.items.clone().into_iter(),
        }
    }
}

impl<T> Unpin for FlowOfRun<T> {}

impl<T> Stream for FlowOfRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.get_mut().items.next())
    }
}
