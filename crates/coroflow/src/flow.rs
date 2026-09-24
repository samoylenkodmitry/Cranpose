use std::{future::Future, pin::Pin, rc::Rc, sync::Arc, time::Duration};

use futures_core::Stream;

use crate::{
    builders::Emitter,
    channel::{Capacity, ProduceIn, Receiver, channel},
    combining::Zip,
    dispatcher::Dispatcher,
    errors::{Catch, RetryWhen},
    flattening::{FlatMap, Flatten},
    job::Job,
    operators::{
        Buffered, Combine, Debounce, DistinctUntilChanged, FLOW_ON_BUFFER, Filter, Map,
        OnCompletion, OnEach, OnStart, StartWith, Take, WholeValue,
    },
    scope::Spawn,
    shaping::{Chunked, OnEmpty, RunningReduce, WithIndex},
    shared::{MutableSharedFlow, SharedFlow},
    sharing::{
        FirstState, SharingStarted, SharingTask, StateInFirst, shared_sharing, state_sharing,
    },
    state::StateFlow,
    suspending::{
        CollectAsync, CollectLatest, FilterMapping, Filtering, InOrder, Inspecting, LatestOnly,
        Mapping, Suspending, SuspendingRun, Transforming, TransformingWhile,
    },
    terminal::{Collect, Count, First, Fold, Last, Reduce, Single, ToVec},
    timing::{Sample, Timeout},
    transforms::{FilterMap, Scan, Skip, Skipping, Taking, While},
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

    /// Accumulates values, emitting `initial` and then every intermediate
    /// result — Kotlin's `scan` / `runningFold`.
    fn scan<A, F>(self, initial: A, step: F) -> Scan<Self, A, F>
    where
        A: Clone,
        F: Fn(&A, Self::Item) -> A + Clone,
    {
        Scan::new(self, initial, step)
    }

    /// Ignores the first `count` values — Kotlin's `drop`.
    fn skip(self, count: usize) -> Skip<Self> {
        Skip::new(self, count)
    }

    /// Transforms each value and drops the `None`s — Kotlin's `mapNotNull`.
    fn filter_map<U, F>(self, transform: F) -> FilterMap<Self, F>
    where
        F: Fn(Self::Item) -> Option<U> + Clone,
    {
        FilterMap::new(self, transform)
    }

    /// Emits the first `count` values, then stops and cancels the upstream.
    fn take(self, count: usize) -> Take<Self> {
        Take::new(self, count)
    }

    /// Emits values while `predicate` holds, then completes and cancels the
    /// upstream — Kotlin's `takeWhile`.
    fn take_while<P>(self, predicate: P) -> While<Self, P, Taking>
    where
        P: Fn(&Self::Item) -> bool + Clone,
    {
        While::new(self, predicate)
    }

    /// Ignores values while `predicate` holds, then emits every value —
    /// Kotlin's `dropWhile`.
    fn skip_while<P>(self, predicate: P) -> While<Self, P, Skipping>
    where
        P: Fn(&Self::Item) -> bool + Clone,
    {
        While::new(self, predicate)
    }

    /// Emits whatever the suspending `action` emits for each value — Kotlin's
    /// `transform`.
    ///
    /// ```
    /// use coroflow::{FlowExt, flow_of};
    /// let doubled = flow_of(vec![1, 2]).transform(async |value, emitter| {
    ///     emitter.emit(value).await;
    ///     emitter.emit(value * 10).await;
    /// });
    /// assert_eq!(pollster::block_on(doubled.to_vec()), vec![1, 10, 2, 20]);
    /// ```
    fn transform<U, F, Fut>(self, action: F) -> Suspending<Self, F, Transforming<U>, InOrder>
    where
        F: FnOnce(Self::Item, Emitter<U>) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        Suspending::new(self, action)
    }

    /// Like [`transform`](FlowExt::transform), but completes and cancels the
    /// upstream as soon as `action` returns `false` — Kotlin's
    /// `transformWhile`.
    fn transform_while<U, F, Fut>(
        self,
        action: F,
    ) -> Suspending<Self, F, TransformingWhile<U>, InOrder>
    where
        F: FnOnce(Self::Item, Emitter<U>) -> Fut + Clone,
        Fut: Future<Output = bool>,
    {
        Suspending::new(self, action)
    }

    /// Like [`transform`](FlowExt::transform), but a newer value cancels the
    /// `action` still running for the previous one — Kotlin's
    /// `transformLatest`.
    fn transform_latest<U, F, Fut>(
        self,
        action: F,
    ) -> Suspending<Self, F, Transforming<U>, LatestOnly>
    where
        F: FnOnce(Self::Item, Emitter<U>) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        Suspending::new(self, action)
    }

    /// Transforms each value with a suspending `transform`, one value at a
    /// time — Kotlin's `map` with a `suspend` lambda.
    ///
    /// The closure is cloned for every value, so capture shared state as an
    /// `Arc` or `Rc` and write it without any `clone()`:
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use coroflow::{FlowExt, flow_of};
    /// struct Repository;
    /// impl Repository {
    ///     async fn load(&self, id: u32) -> String {
    ///         format!("note {id}")
    ///     }
    /// }
    /// let repository = Arc::new(Repository);
    /// let notes = flow_of(vec![1, 2]).map_async(async move |id| repository.load(id).await);
    /// assert_eq!(pollster::block_on(notes.to_vec()), vec!["note 1", "note 2"]);
    /// ```
    fn map_async<F, Fut>(self, transform: F) -> Suspending<Self, F, Mapping, InOrder>
    where
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future,
    {
        Suspending::new(self, transform)
    }

    /// Like [`map_async`](FlowExt::map_async), but a newer value cancels the
    /// transformation still running for the previous one — Kotlin's
    /// `mapLatest`.
    fn map_latest<F, Fut>(self, transform: F) -> Suspending<Self, F, Mapping, LatestOnly>
    where
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future,
    {
        Suspending::new(self, transform)
    }

    /// Keeps the values a suspending `predicate` accepts — Kotlin's `filter`
    /// with a `suspend` lambda. The predicate gets a clone of each value.
    fn filter_async<F, Fut>(self, predicate: F) -> Suspending<Self, F, Filtering, InOrder>
    where
        Self::Item: Clone,
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future<Output = bool>,
    {
        Suspending::new(self, predicate)
    }

    /// Transforms each value with a suspending `transform` and drops the
    /// `None`s — Kotlin's `mapNotNull` with a `suspend` lambda.
    fn filter_map_async<U, F, Fut>(
        self,
        transform: F,
    ) -> Suspending<Self, F, FilterMapping, InOrder>
    where
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future<Output = Option<U>>,
    {
        Suspending::new(self, transform)
    }

    /// Runs a suspending `action` on a clone of each value before emitting it
    /// — Kotlin's `onEach` with a `suspend` lambda.
    fn on_each_async<F, Fut>(self, action: F) -> Suspending<Self, F, Inspecting, InOrder>
    where
        Self::Item: Clone,
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        Suspending::new(self, action)
    }

    /// Drops values equal to the one emitted just before.
    fn distinct_until_changed(self) -> DistinctUntilChanged<Self, WholeValue>
    where
        Self::Item: PartialEq + Clone,
    {
        DistinctUntilChanged::new(self, WholeValue)
    }

    /// Drops values whose `key` equals the key of the value emitted just
    /// before — Kotlin's `distinctUntilChangedBy`.
    fn distinct_until_changed_by<K, F>(self, key: F) -> DistinctUntilChanged<Self, F>
    where
        K: PartialEq,
        F: Fn(&Self::Item) -> K + Clone,
    {
        DistinctUntilChanged::new(self, key)
    }

    /// Pairs every value with its position, counting from zero — Kotlin's
    /// `withIndex`.
    fn with_index(self) -> WithIndex<Self> {
        WithIndex::new(self)
    }

    /// Emits the first value, then each value folded into the one emitted
    /// before — Kotlin's `runningReduce`.
    fn running_reduce<R>(self, reducer: R) -> RunningReduce<Self, R>
    where
        Self::Item: Clone,
        R: Fn(&Self::Item, Self::Item) -> Self::Item + Clone,
    {
        RunningReduce::new(self, reducer)
    }

    /// Groups values into vectors of `size`; the last one may be shorter —
    /// Kotlin's `chunked`. A `size` of zero counts as one.
    fn chunked(self, size: usize) -> Chunked<Self> {
        Chunked::new(self, size)
    }

    /// Emits the latest value once per `period`, skipping periods without a
    /// new value — Kotlin's `sample`. A value still waiting when the
    /// upstream completes is dropped.
    fn sample(self, period: Duration) -> Sample<Self> {
        Sample::new(self, period)
    }

    /// Wraps values in `Ok`, and emits `Err(TimedOut)` and completes if the
    /// upstream goes `limit` without emitting — Kotlin's `timeout`.
    fn timeout(self, limit: Duration) -> Timeout<Self> {
        Timeout::new(self, limit)
    }

    /// Runs the suspending `action` if the flow completes without emitting,
    /// so it can emit a fallback — Kotlin's `onEmpty`.
    fn on_empty<F, Fut>(self, action: F) -> OnEmpty<Self, F>
    where
        F: FnOnce(Emitter<Self::Item>) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        OnEmpty::new(self, action)
    }

    /// Emits a value only after `timeout` passes without a newer one.
    fn debounce(self, timeout: Duration) -> Debounce<Self> {
        Debounce::new(self, timeout)
    }

    /// Maps each value to a flow and emits from the newest one, cancelling the
    /// run of the previous flow — Kotlin's `flatMapLatest`.
    fn flat_map_latest<G, F>(self, transform: F) -> FlatMap<Self, F>
    where
        G: Flow,
        F: Fn(Self::Item) -> G + Clone,
    {
        FlatMap::latest(self, transform)
    }

    /// Maps each value to a flow and emits all of it before taking the next
    /// value — Kotlin's `flatMapConcat`.
    fn flat_map_concat<G, F>(self, transform: F) -> FlatMap<Self, F>
    where
        G: Flow,
        F: Fn(Self::Item) -> G + Clone,
    {
        FlatMap::concurrent(self, transform, 1)
    }

    /// Collects each inner flow in turn — Kotlin's `flattenConcat`.
    fn flatten_concat(self) -> Flatten<Self>
    where
        Self::Item: Flow,
    {
        FlatMap::concurrent(self, std::convert::identity, 1)
    }

    /// Collects up to `concurrency` inner flows at once, emitting their
    /// values as they arrive — Kotlin's `flattenMerge`; Kotlin's default
    /// concurrency is [`DEFAULT_CONCURRENCY`](crate::DEFAULT_CONCURRENCY).
    fn flatten_merge(self, concurrency: usize) -> Flatten<Self>
    where
        Self::Item: Flow,
    {
        FlatMap::concurrent(self, std::convert::identity, concurrency)
    }

    /// Maps each value to a flow and runs up to `concurrency` of them at once,
    /// emitting values as they arrive — Kotlin's `flatMapMerge`.
    fn flat_map_merge<G, F>(self, concurrency: usize, transform: F) -> FlatMap<Self, F>
    where
        G: Flow,
        F: Fn(Self::Item) -> G + Clone,
    {
        FlatMap::concurrent(self, transform, concurrency)
    }

    /// Pairs the n-th value of this flow with the n-th value of `other`,
    /// completing when either completes — Kotlin's `zip`.
    fn zip<O, U, F>(self, other: O, combiner: F) -> Zip<Self, O, F>
    where
        O: Flow,
        F: Fn(Self::Item, O::Item) -> U + Clone,
    {
        Zip::new(self, other, combiner)
    }

    /// Combines the latest values of both flows once each has emitted.
    fn combine<O, U, F>(self, other: O, combiner: F) -> Combine<Self, O, F>
    where
        O: Flow,
        F: Fn(&Self::Item, &O::Item) -> U + Clone,
    {
        Combine::new(self, other, combiner)
    }

    /// On the first `Err`, stops the upstream and continues with the flow
    /// `handler` returns for the error — Kotlin's `catch`.
    fn catch<T, E, G, H>(self, handler: H) -> Catch<Self, H>
    where
        Self: Flow<Item = Result<T, E>>,
        G: Flow<Item = T>,
        H: Fn(E) -> G + Clone,
    {
        Catch::new(self, handler)
    }

    /// Runs the upstream again after an `Err`, up to `attempts` more times,
    /// and passes the last error on — Kotlin's `retry`.
    fn retry<T, E>(
        self,
        attempts: u32,
    ) -> RetryWhen<Self, impl Fn(&E, u32) -> Option<Duration> + Clone>
    where
        Self: Flow<Item = Result<T, E>> + Clone,
    {
        RetryWhen::new(self, move |_: &E, attempt: u32| {
            (attempt < attempts).then_some(Duration::ZERO)
        })
    }

    /// Runs the upstream again after an `Err` when `policy` returns a back-off
    /// delay for that error and attempt, and passes the error on when it
    /// returns `None` — Kotlin's `retryWhen`.
    fn retry_when<T, E, P>(self, policy: P) -> RetryWhen<Self, P>
    where
        Self: Flow<Item = Result<T, E>> + Clone,
        P: Fn(&E, u32) -> Option<Duration> + Clone,
    {
        RetryWhen::new(self, policy)
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
    fn flow_on(self, dispatcher: Dispatcher) -> Buffered<Self> {
        Buffered::new(self, Some(dispatcher), Capacity::Buffered(FLOW_ON_BUFFER))
    }

    /// Runs the upstream in its own coroutine on the collector's dispatcher,
    /// letting it get up to `capacity` values ahead of a slow collector —
    /// Kotlin's `buffer`.
    fn buffer(self, capacity: usize) -> Buffered<Self> {
        Buffered::new(self, None, Capacity::Buffered(capacity))
    }

    /// Runs the upstream in its own coroutine and hands a slow collector only
    /// the newest value — Kotlin's `conflate`.
    fn conflate(self) -> Buffered<Self> {
        Buffered::new(self, None, Capacity::Conflated)
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
        S: Spawn<SharingTask<Self, StateFlow<Self::Item>>>,
        Self::Item: Clone + PartialEq,
    {
        let (task, state) = state_sharing(self, started, initial);
        scope.spawn(task);
        state
    }

    /// Starts this flow in `scope` at once and waits for its first value,
    /// then returns a [`StateFlow`] that starts from it — Kotlin's suspending
    /// `stateIn(scope)`. `None` if the flow completes without emitting.
    fn state_in_first<S>(self, scope: &S) -> StateInFirst<Self::Item>
    where
        S: Spawn<FirstState<Self::Run>>,
        Self::Item: Clone + PartialEq,
    {
        let (task, state) = FirstState::start(self.open());
        scope.spawn(task);
        state
    }

    /// Shares this flow as a hot [`SharedFlow`] that runs in `scope` and
    /// replays the last `replay` values to new collectors — Kotlin's
    /// `shareIn`. Collectors may fall up to
    /// [`SHARE_IN_BUFFER`](crate::SHARE_IN_BUFFER) values or `replay`,
    /// whichever is larger, behind; beyond that the upstream waits for them.
    fn share_in<S>(
        self,
        scope: &S,
        started: SharingStarted,
        replay: usize,
    ) -> SharedFlow<Self::Item>
    where
        S: Spawn<SharingTask<Self, MutableSharedFlow<Self::Item>>>,
        Self::Item: Clone,
    {
        let (task, events) = shared_sharing(self, started, replay);
        scope.spawn(task);
        events
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

    /// Runs the flow, awaiting a suspending `action` for every value in turn,
    /// until it completes — Kotlin's `collect` with a `suspend` lambda.
    fn collect_async<F, Fut>(&self, action: F) -> CollectAsync<Self::Run, F>
    where
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        Collect::new(SuspendingRun::new(self.open(), action), drop)
    }

    /// Runs the flow, cancelling the `action` still running for a value as
    /// soon as a newer value arrives — Kotlin's `collectLatest`.
    fn collect_latest<F, Fut>(&self, action: F) -> CollectLatest<Self::Run, F>
    where
        F: FnOnce(Self::Item) -> Fut + Clone,
        Fut: Future<Output = ()>,
    {
        Collect::new(SuspendingRun::new(self.open(), action), drop)
    }

    /// Collects the flow in `scope`, ignoring its values — Kotlin's
    /// `launchIn`, usually after [`on_each`](FlowExt::on_each).
    fn launch_in<S>(&self, scope: &S) -> Job
    where
        S: Spawn<Collect<Self::Run, fn(Self::Item)>>,
    {
        scope.spawn(Collect::new(self.open(), drop))
    }

    /// Collects the flow in `scope` into a buffered channel and returns its
    /// receiving end — Kotlin's `produceIn`. Dropping every receiver cancels
    /// the collection.
    fn produce_in<S>(&self, scope: &S) -> Receiver<Self::Item>
    where
        S: Spawn<ProduceIn<Self::Run>>,
    {
        let (sender, receiver) = channel(Capacity::BUFFERED);
        scope.spawn(ProduceIn::new(self.open(), sender));
        receiver
    }

    /// Runs the flow, folding every value into an accumulator that starts at
    /// `initial` — Kotlin's `fold`.
    fn fold<A, F>(&self, initial: A, step: F) -> Fold<Self::Run, A, F>
    where
        F: FnMut(A, Self::Item) -> A,
    {
        Fold::new(self.open(), initial, step)
    }

    /// Runs the flow, folding every value into the first one; `None` for an
    /// empty flow — Kotlin's `reduceOrNull`.
    fn reduce<F>(&self, reducer: F) -> Reduce<Self::Run, F>
    where
        F: FnMut(Self::Item, Self::Item) -> Self::Item,
    {
        Reduce::new(self.open(), reducer)
    }

    /// Runs the flow and counts its values — Kotlin's `count`.
    fn count(&self) -> Count<Self::Run, Self::Item> {
        Fold::new(self.open(), 0, |count, _| count + 1)
    }

    /// Runs the flow and returns its last value — Kotlin's `lastOrNull`.
    fn last(&self) -> Last<Self::Run, Self::Item> {
        Fold::new(self.open(), None, |_, value| Some(value))
    }

    /// Returns the flow's only value, or `None` once it turns out to have
    /// none or more than one — Kotlin's `singleOrNull`.
    fn single(&self) -> Single<Self::Run> {
        Single::new(self.open())
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
