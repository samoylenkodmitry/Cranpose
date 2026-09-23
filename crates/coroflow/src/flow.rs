use std::{pin::Pin, rc::Rc, sync::Arc, time::Duration};

use futures_core::Stream;

use crate::{
    dispatcher::Dispatcher,
    operators::{
        Combine, Debounce, DistinctUntilChanged, Filter, FlatMapLatest, FlowOn, Map, OnCompletion,
        OnEach, OnStart, StartWith, Take,
    },
    scope::Spawn,
    sharing::{SharingStarted, SharingTask},
    state::StateFlow,
    terminal::{Collect, First, ToVec},
};

/// A cold, re-runnable asynchronous sequence — Kotlin's `Flow`.
///
/// A `Flow` is a recipe. [`open`](Flow::open) starts one independent run and
/// returns it as a [`Stream`]; collecting twice runs the recipe twice, and
/// dropping a run cancels whatever it was doing. Operators compile into plain
/// structs, so a chain allocates nothing per emitted item, and the compiler
/// infers whether a flow is `Send` from what it captures.
pub trait Flow {
    /// The type of the values this flow emits.
    type Item;
    /// One run of this flow.
    type Run: Stream<Item = Self::Item> + Unpin;

    /// Starts a new, independent run.
    fn open(&self) -> Self::Run;
}

/// A [`Flow`] that background scopes can run — what data and domain layers
/// return as `impl SendFlow<Item = T>`.
///
/// An `impl Flow<Item = T>` return type hides whether its runs are `Send`;
/// this alias keeps that fact visible without boxing.
pub trait SendFlow: Flow<Run: Send + 'static> + Send + Sync + 'static {}

impl<F> SendFlow for F
where
    F: Flow + Send + Sync + 'static,
    F::Run: Send + 'static,
{
}

/// A type-erased, shareable [`Flow`] that can cross threads.
///
/// Use it at the boundaries where Kotlin code writes `Flow<T>` in an interface,
/// such as a repository trait.
pub struct BoxFlow<T> {
    inner: Arc<dyn ErasedFlow<T> + Send + Sync>,
}

trait ErasedFlow<T> {
    fn open_boxed(&self) -> Pin<Box<dyn Stream<Item = T> + Send>>;
}

impl<F> ErasedFlow<F::Item> for F
where
    F: Flow,
    F::Run: Send + 'static,
{
    fn open_boxed(&self) -> Pin<Box<dyn Stream<Item = F::Item> + Send>> {
        Box::pin(self.open())
    }
}

impl<T> Clone for BoxFlow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Flow for BoxFlow<T> {
    type Item = T;
    type Run = Pin<Box<dyn Stream<Item = T> + Send>>;

    fn open(&self) -> Self::Run {
        self.inner.open_boxed()
    }
}

/// A type-erased [`Flow`] confined to one thread, for flows that capture
/// `Rc` or other thread-bound values.
pub struct LocalBoxFlow<T> {
    inner: Rc<dyn LocalErasedFlow<T>>,
}

trait LocalErasedFlow<T> {
    fn open_boxed(&self) -> Pin<Box<dyn Stream<Item = T>>>;
}

impl<F> LocalErasedFlow<F::Item> for F
where
    F: Flow,
    F::Run: 'static,
{
    fn open_boxed(&self) -> Pin<Box<dyn Stream<Item = F::Item>>> {
        Box::pin(self.open())
    }
}

impl<T> Clone for LocalBoxFlow<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Flow for LocalBoxFlow<T> {
    type Item = T;
    type Run = Pin<Box<dyn Stream<Item = T>>>;

    fn open(&self) -> Self::Run {
        self.inner.open_boxed()
    }
}

/// Operators and terminal operations available on every [`Flow`].
pub trait FlowExt: Flow + Sized {
    /// Transforms each value.
    fn map<U, F>(self, transform: F) -> Map<Self, F>
    where
        F: Fn(Self::Item) -> U + Clone,
    {
        Map::new(self, transform)
    }

    /// Keeps only the values `predicate` accepts.
    fn filter<F>(self, predicate: F) -> Filter<Self, F>
    where
        F: Fn(&Self::Item) -> bool + Clone,
    {
        Filter::new(self, predicate)
    }

    /// Runs `action` for each value before passing it on.
    fn on_each<F>(self, action: F) -> OnEach<Self, F>
    where
        F: Fn(&Self::Item) + Clone,
    {
        OnEach::new(self, action)
    }

    /// Runs `action` each time a collection starts — Kotlin's `onStart`.
    fn on_start<F>(self, action: F) -> OnStart<Self, F>
    where
        F: Fn(),
    {
        OnStart::new(self, action)
    }

    /// Runs `action` when a collection ends, whether the upstream completed or
    /// the collector was cancelled — Kotlin's `onCompletion`.
    fn on_completion<F>(self, action: F) -> OnCompletion<Self, F>
    where
        F: Fn() + Clone,
    {
        OnCompletion::new(self, action)
    }

    /// Emits `first` before the upstream's values — Kotlin's
    /// `onStart { emit(first) }`.
    fn start_with(self, first: Self::Item) -> StartWith<Self>
    where
        Self::Item: Clone,
    {
        StartWith::new(self, first)
    }

    /// Emits the first `count` values, then stops and cancels the upstream.
    fn take(self, count: usize) -> Take<Self> {
        Take::new(self, count)
    }

    /// Drops values equal to the one emitted just before.
    fn distinct_until_changed(self) -> DistinctUntilChanged<Self>
    where
        Self::Item: PartialEq + Clone,
    {
        DistinctUntilChanged::new(self)
    }

    /// Emits a value only after `timeout` passes without a newer one.
    fn debounce(self, timeout: Duration) -> Debounce<Self> {
        Debounce::new(self, timeout)
    }

    /// Maps each value to a flow and emits from the newest one, cancelling the
    /// run of the previous flow — Kotlin's `flatMapLatest`.
    fn flat_map_latest<G, F>(self, transform: F) -> FlatMapLatest<Self, F>
    where
        G: Flow,
        F: Fn(Self::Item) -> G + Clone,
    {
        FlatMapLatest::new(self, transform)
    }

    /// Combines the latest values of both flows once each has emitted.
    fn combine<O, U, F>(self, other: O, combiner: F) -> Combine<Self, O, F>
    where
        O: Flow,
        F: Fn(&Self::Item, &O::Item) -> U + Clone,
    {
        Combine::new(self, other, combiner)
    }

    /// Runs this flow's upstream on `dispatcher` — Kotlin's `flowOn`.
    ///
    /// Values cross threads through a buffer of
    /// [`FLOW_ON_BUFFER`](crate::FLOW_ON_BUFFER) items. The upstream must be
    /// `Send`:
    ///
    /// ```compile_fail
    /// use std::rc::Rc;
    /// use coroflow::{flow_of, Dispatchers, Flow, FlowExt};
    /// let local = Rc::new(1);
    /// let flow = flow_of(vec![1]).map(move |value| value + *local);
    /// let _ = flow.flow_on(Dispatchers::default_pool()).open();
    /// ```
    fn flow_on(self, dispatcher: Dispatcher) -> FlowOn<Self> {
        FlowOn::new(self, dispatcher)
    }

    /// Shares this flow as a hot [`StateFlow`] that runs in `scope` —
    /// Kotlin's `stateIn`.
    ///
    /// `started` decides when the upstream runs. With a [`MainScope`](crate::MainScope)
    /// the upstream may hold thread-bound values; with a
    /// [`CoroutineScope`](crate::CoroutineScope) it must be `Send`, which the
    /// compiler checks through [`Spawn`].
    fn state_in<S>(
        self,
        scope: &S,
        started: SharingStarted,
        initial: Self::Item,
    ) -> StateFlow<Self::Item>
    where
        S: Spawn<SharingTask<Self>>,
        Self::Item: Clone + PartialEq,
    {
        let (task, state) = SharingTask::new(self, started, initial);
        scope.spawn(task);
        state
    }

    /// Erases the type so the flow can sit behind a trait or be stored.
    fn boxed(self) -> BoxFlow<Self::Item>
    where
        Self: Send + Sync + 'static,
        Self::Run: Send + 'static,
    {
        BoxFlow {
            inner: Arc::new(self),
        }
    }

    /// Erases the type of a flow that stays on one thread.
    fn boxed_local(self) -> LocalBoxFlow<Self::Item>
    where
        Self: 'static,
        Self::Run: 'static,
    {
        LocalBoxFlow {
            inner: Rc::new(self),
        }
    }

    /// Runs the flow, calling `action` for every value, until it completes.
    fn collect<F>(&self, action: F) -> Collect<Self::Run, F>
    where
        F: FnMut(Self::Item),
    {
        Collect::new(self.open(), action)
    }

    /// Runs the flow until its first value and returns it.
    fn first(&self) -> First<Self::Run> {
        First::new(self.open())
    }

    /// Runs the flow to completion and gathers every value.
    fn to_vec(&self) -> ToVec<Self::Run> {
        ToVec::new(self.open())
    }
}

impl<T: Flow> FlowExt for T {}
