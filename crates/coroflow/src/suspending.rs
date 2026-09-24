use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll, ready},
};

use futures_core::Stream;

use crate::{builders::Emitter, flow::Flow, operators::poll_run, terminal::Collect};

/// What a suspending operator does once its action for one value finished.
pub enum Finish<T> {
    /// Emit this value downstream.
    Emit(T),
    /// Emit nothing for this value.
    Skip,
    /// Complete the flow and cancel the upstream.
    Stop,
}

/// How one suspending operator calls its action and what it emits.
///
/// Each suspending operator of [`FlowExt`](crate::FlowExt) is a
/// [`Suspending`] flow with its own step: [`Mapping`], [`Filtering`],
/// [`FilterMapping`], [`Inspecting`], [`Transforming`] or
/// [`TransformingWhile`].
pub trait Step<T, F> {
    /// What the operator emits.
    type Item;
    /// What the operator keeps while the action runs.
    type Held;
    /// The action's future.
    type Action: Future;

    /// Starts the action for `value`. `emitter` is created on first use by
    /// the steps that emit from inside their action.
    fn start(
        action: F,
        value: T,
        emitter: &mut Option<Emitter<Self::Item>>,
    ) -> (Self::Action, Self::Held);

    /// Decides what the finished action means downstream.
    fn finish(held: Self::Held, output: <Self::Action as Future>::Output) -> Finish<Self::Item>;
}

/// The step of [`map_async`](crate::FlowExt::map_async),
/// [`map_latest`](crate::FlowExt::map_latest),
/// [`collect_async`](crate::FlowExt::collect_async) and
/// [`collect_latest`](crate::FlowExt::collect_latest).
pub struct Mapping;

/// The step of [`filter_map_async`](crate::FlowExt::filter_map_async).
pub struct FilterMapping;

/// What a suspending action's result says about its value: `bool` keeps
/// the value when `true`, and `()` always keeps it.
pub trait Verdict {
    /// Whether the value is kept, or the flow goes on.
    fn keeps(self) -> bool;
}

impl Verdict for bool {
    fn keeps(self) -> bool {
        self
    }
}

impl Verdict for () {
    fn keeps(self) -> bool {
        true
    }
}

/// The step that runs an action on a clone of each value and emits the
/// value when the action's [`Verdict`] keeps it.
pub struct Passing<V>(PhantomData<fn() -> V>);

/// The step of [`filter_async`](crate::FlowExt::filter_async).
pub type Filtering = Passing<bool>;

/// The step of [`on_each_async`](crate::FlowExt::on_each_async).
pub type Inspecting = Passing<()>;

/// The step that hands each value and an [`Emitter`] to an action, and
/// completes the flow when the action's [`Verdict`] says not to go on.
pub struct Emitting<U, V>(PhantomData<fn() -> (U, V)>);

/// The step of [`transform`](crate::FlowExt::transform) and
/// [`transform_latest`](crate::FlowExt::transform_latest).
pub type Transforming<U> = Emitting<U, ()>;

/// The step of [`transform_while`](crate::FlowExt::transform_while).
pub type TransformingWhile<U> = Emitting<U, bool>;

impl<T, F, Fut> Step<T, F> for Mapping
where
    F: FnOnce(T) -> Fut,
    Fut: Future,
{
    type Item = Fut::Output;
    type Held = ();
    type Action = Fut;

    fn start(action: F, value: T, _: &mut Option<Emitter<Fut::Output>>) -> (Fut, ()) {
        (action(value), ())
    }

    fn finish((): (), output: Fut::Output) -> Finish<Fut::Output> {
        Finish::Emit(output)
    }
}

impl<T, U, F, Fut> Step<T, F> for FilterMapping
where
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = Option<U>>,
{
    type Item = U;
    type Held = ();
    type Action = Fut;

    fn start(action: F, value: T, _: &mut Option<Emitter<U>>) -> (Fut, ()) {
        (action(value), ())
    }

    fn finish((): (), output: Option<U>) -> Finish<U> {
        output.map_or(Finish::Skip, Finish::Emit)
    }
}

impl<T, V, F, Fut> Step<T, F> for Passing<V>
where
    T: Clone,
    V: Verdict,
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = V>,
{
    type Item = T;
    type Held = T;
    type Action = Fut;

    fn start(action: F, value: T, _: &mut Option<Emitter<T>>) -> (Fut, T) {
        (action(value.clone()), value)
    }

    fn finish(held: T, verdict: V) -> Finish<T> {
        if verdict.keeps() {
            Finish::Emit(held)
        } else {
            Finish::Skip
        }
    }
}

impl<T, U, V, F, Fut> Step<T, F> for Emitting<U, V>
where
    V: Verdict,
    F: FnOnce(T, Emitter<U>) -> Fut,
    Fut: Future<Output = V>,
{
    type Item = U;
    type Held = ();
    type Action = Fut;

    fn start(action: F, value: T, emitter: &mut Option<Emitter<U>>) -> (Fut, ()) {
        let emitter = emitter.get_or_insert_with(Emitter::new).clone();
        (action(value, emitter), ())
    }

    fn finish((): (), verdict: V) -> Finish<U> {
        if verdict.keeps() {
            Finish::Skip
        } else {
            Finish::Stop
        }
    }
}

/// Runs the action for one value at a time, in order.
pub struct InOrder;

/// Cancels the running action as soon as a newer value arrives — Kotlin's
/// `*Latest` operators.
pub struct LatestOnly;

/// A flow whose operator runs a suspending action per value; see [`Step`].
pub struct Suspending<S, F, M, K> {
    upstream: S,
    action: F,
    _step: PhantomData<fn() -> (M, K)>,
}

impl<S, F, M, K> Suspending<S, F, M, K> {
    pub(crate) fn new(upstream: S, action: F) -> Self {
        Self {
            upstream,
            action,
            _step: PhantomData,
        }
    }
}

impl<S: Clone, F: Clone, M, K> Clone for Suspending<S, F, M, K> {
    fn clone(&self) -> Self {
        Self::new(self.upstream.clone(), self.action.clone())
    }
}

struct Running<Fut> {
    slot: Option<Pin<Box<Option<Fut>>>>,
}

impl<Fut: Future> Running<Fut> {
    fn start(&mut self, action: Fut) {
        match self.slot.as_mut() {
            Some(slot) => slot.as_mut().set(Some(action)),
            None => self.slot = Some(Box::pin(Some(action))),
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Fut::Output>> {
        let Some(slot) = self.slot.as_mut() else {
            return Poll::Ready(None);
        };
        let Some(action) = slot.as_mut().as_pin_mut() else {
            return Poll::Ready(None);
        };
        let output = ready!(action.poll(cx));
        slot.as_mut().set(None);
        Poll::Ready(Some(output))
    }
}

struct Actions<T, F, M: Step<T, F>> {
    action: F,
    running: Running<M::Action>,
    held: Option<M::Held>,
    emitter: Option<Emitter<M::Item>>,
    finished: Option<Finish<M::Item>>,
}

impl<T, F: Clone, M: Step<T, F>> Actions<T, F, M> {
    fn new(action: F) -> Self {
        Self {
            action,
            running: Running { slot: None },
            held: None,
            emitter: None,
            finished: None,
        }
    }

    fn start(&mut self, value: T) {
        let (action, held) = M::start(self.action.clone(), value, &mut self.emitter);
        self.running.start(action);
        self.held = Some(held);
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Finish<M::Item>>> {
        let polled = self.running.poll(cx);
        let running = polled.is_pending();
        if let Poll::Ready(Some(output)) = polled
            && let Some(held) = self.held.take()
        {
            self.finished = Some(M::finish(held, output));
        }
        if let Some(value) = self.emitter.as_ref().and_then(Emitter::take_emitted) {
            return Poll::Ready(Some(Finish::Emit(value)));
        }
        if let Some(finished) = self.finished.take() {
            return Poll::Ready(Some(finished));
        }
        if running {
            Poll::Pending
        } else {
            Poll::Ready(None)
        }
    }
}

/// The future returned by [`collect_async`](crate::FlowExt::collect_async).
pub type CollectAsync<R, F> = Collect<SuspendingRun<R, F, Mapping, InOrder>, fn(())>;

/// The future returned by [`collect_latest`](crate::FlowExt::collect_latest).
pub type CollectLatest<R, F> = Collect<SuspendingRun<R, F, Mapping, LatestOnly>, fn(())>;

/// One run of a [`Suspending`] flow.
pub struct SuspendingRun<R, F, M, K>
where
    R: Stream,
    M: Step<R::Item, F>,
{
    upstream: Option<R>,
    actions: Actions<R::Item, F, M>,
    _kind: PhantomData<fn() -> K>,
}

impl<R, F, M, K> SuspendingRun<R, F, M, K>
where
    R: Stream,
    F: Clone,
    M: Step<R::Item, F>,
{
    pub(crate) fn new(upstream: R, action: F) -> Self {
        Self {
            upstream: Some(upstream),
            actions: Actions::new(action),
            _kind: PhantomData,
        }
    }
}

impl<R, F, M, K> Unpin for SuspendingRun<R, F, M, K>
where
    R: Stream,
    M: Step<R::Item, F>,
{
}

impl<S, F, M, K> Flow for Suspending<S, F, M, K>
where
    S: Flow,
    F: Clone,
    M: Step<S::Item, F>,
    K: Order,
{
    type Item = M::Item;
    type Run = SuspendingRun<S::Run, F, M, K>;

    fn open(&self) -> Self::Run {
        SuspendingRun::new(self.upstream.open(), self.action.clone())
    }
}

/// How a [`Suspending`] flow schedules its actions: [`InOrder`] or
/// [`LatestOnly`].
pub trait Order {
    /// Whether a newer value cancels the action still running.
    const LATEST_ONLY: bool;
}

impl Order for InOrder {
    const LATEST_ONLY: bool = false;
}

impl Order for LatestOnly {
    const LATEST_ONLY: bool = true;
}

impl<R, F, M, K> SuspendingRun<R, F, M, K>
where
    R: Stream + Unpin,
    F: Clone,
    M: Step<R::Item, F>,
{
    fn poll_in_order(&mut self, cx: &mut Context<'_>) -> Poll<Option<M::Item>> {
        loop {
            match self.actions.poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Finish::Emit(value))) => return Poll::Ready(Some(value)),
                Poll::Ready(Some(Finish::Stop)) => {
                    self.upstream = None;
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Finish::Skip) | None) => {}
            }
            let Some(upstream) = self.upstream.as_mut() else {
                return Poll::Ready(None);
            };
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => self.actions.start(value),
                Poll::Ready(None) => {
                    self.upstream = None;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn poll_latest(&mut self, cx: &mut Context<'_>) -> Poll<Option<M::Item>> {
        loop {
            if let Some(upstream) = self.upstream.as_mut() {
                match poll_run(upstream, cx) {
                    Poll::Ready(Some(value)) => {
                        self.actions.start(value);
                        continue;
                    }
                    Poll::Ready(None) => self.upstream = None,
                    Poll::Pending => {}
                }
            }
            match self.actions.poll(cx) {
                Poll::Ready(Some(Finish::Emit(value))) => return Poll::Ready(Some(value)),
                Poll::Ready(Some(Finish::Skip)) => {}
                Poll::Ready(Some(Finish::Stop)) => {
                    self.upstream = None;
                    return Poll::Ready(None);
                }
                Poll::Ready(None) if self.upstream.is_none() => return Poll::Ready(None),
                Poll::Ready(None) | Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<R, F, M, K> Stream for SuspendingRun<R, F, M, K>
where
    R: Stream + Unpin,
    F: Clone,
    M: Step<R::Item, F>,
    K: Order,
{
    type Item = M::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<M::Item>> {
        let this = self.get_mut();
        if K::LATEST_ONLY {
            this.poll_latest(cx)
        } else {
            this.poll_in_order(cx)
        }
    }
}
