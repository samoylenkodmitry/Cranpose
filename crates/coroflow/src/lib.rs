//! Kotlin-style coroutines and `Flow` for Rust, independent of any async
//! runtime and of any UI framework.
//!
//! The pieces map one to one onto `kotlinx.coroutines`:
//!
//! | Kotlin | coroflow |
//! |---|---|
//! | `CoroutineDispatcher` | [`Dispatch`] + [`Dispatcher`] |
//! | `Dispatchers.Default` / `IO` | [`Dispatchers::default_pool`] / [`Dispatchers::io`] |
//! | `Dispatchers.Main`, `viewModelScope` | [`ConfinedDispatcher`], [`MainScope`] |
//! | `CoroutineScope(SupervisorJob())` | [`CoroutineScope`] |
//! | `launch`, `async`, `Job`, `Deferred` | `scope.launch(..)`, `scope.async_(..)`, [`Job`], [`Deferred`] |
//! | `CoroutineStart.LAZY`, `invokeOnCompletion`, `cancelAndJoin`, `joinAll`, `awaitAll`, `CoroutineExceptionHandler` | `launch_lazy`, `async_lazy`, [`Job::invoke_on_completion`], [`Job::cancel_and_join`], [`join_all`], [`await_all`], [`Scope::with_exception_handler`] |
//! | `withContext`, `delay`, `withTimeoutOrNull`, `yield` | [`with_context`], [`delay`], [`with_timeout`], [`yield_now`] |
//! | `coroutineScope`, `supervisorScope`, `select` | [`coroutine_scope`], [`supervisor_scope`], [`select`] |
//! | `Flow`, `flow { }`, `flowOf` | [`Flow`], [`flow`], [`flow_of`] |
//! | `suspend fun` in an interface | a method returning [`BoxFuture`] |
//! | `map`, `filter`, `mapNotNull`, `scan`, `drop`, `debounce`, `zip`, `combine`, `flowOn` | [`FlowExt`] |
//! | `flatMapLatest`, `flatMapConcat`, `flatMapMerge`, `catch`, `retry`, `retryWhen` | [`FlowExt`] |
//! | `sample`, `timeout`, `onEmpty`, `withIndex`, `distinctUntilChangedBy`, `runningReduce`, `chunked`, `flattenConcat`, `flattenMerge` | [`FlowExt`] |
//! | `launchIn`, `produceIn`, `fold`, `reduceOrNull`, `count`, `lastOrNull`, `singleOrNull`, `emitAll` | [`FlowExt`], [`Emitter::emit_all`] |
//! | `transform`, `transformWhile`, `transformLatest`, `mapLatest`, `collectLatest`, `takeWhile`, `dropWhile` | [`FlowExt`] |
//! | a `suspend` lambda in `map`, `filter`, `mapNotNull`, `onEach`, `collect` | [`FlowExt::map_async`], [`FlowExt::filter_async`], [`FlowExt::filter_map_async`], [`FlowExt::on_each_async`], [`FlowExt::collect_async`] |
//! | `merge(a, b)`, `combine(a, b, c)`, `combine(flows)` | [`merge`], [`combine3`], [`combine4`], [`combine5`], [`combine_all`] |
//! | `Mutex`, `Semaphore` | [`Mutex`], [`Semaphore`] |
//! | `Channel`, `produce`, `receiveAsFlow` | [`channel`], [`produce`], [`Receiver`] |
//! | `channelFlow`, `callbackFlow`, `awaitClose` | [`channel_flow`], [`callback_flow`], [`Producer::await_close`] |
//! | `MutableStateFlow`, `MutableSharedFlow`, `BufferOverflow` | [`MutableStateFlow`], [`MutableSharedFlow`], [`BufferOverflow`] |
//! | `stateIn(scope, WhileSubscribed(5000), x)`, `shareIn`, suspending `stateIn(scope)` | [`FlowExt::state_in`], [`FlowExt::share_in`], [`FlowExt::state_in_first`], [`SharingStarted`], [`SharingCommand`] |
//! | `runTest`, `backgroundScope`, `advanceTimeBy`, `advanceUntilIdle` | [`run_test`], [`TestScope`], [`TestScheduler`] |
//! | Turbine's `awaitItem`, `awaitComplete`, `expectNoEvents`, `skipItems` | [`TestScheduler::turbine`], [`Turbine`] |
//!
//! It needs no async runtime. On native targets [`Dispatchers`] are thread
//! pools and [`SystemClock`] is one timer thread; in the browser both run on the
//! page's event loop and timers are `setTimeout`s. Any other executor, such as
//! tokio or a UI toolkit's main loop, plugs in by implementing [`Dispatch`].
//!
//! Three things differ on purpose. A coroutine is cancelled by dropping its
//! future, so there is no `isActive` to check. Whether a flow or coroutine may
//! cross threads is inferred by the compiler from what it captures: background
//! scopes demand `Send`, the main-thread scope does not, and nothing else is
//! annotated. Operators are plain structs, so a chain allocates once per
//! collection and never per emitted value.

/// Kotlin's default number of inner flows [`FlowExt::flat_map_merge`] and
/// [`FlowExt::flatten_merge`] collect at once.
pub const DEFAULT_CONCURRENCY: usize = 16;

mod builders;
mod channel;
mod clock;
mod combining;
mod dispatcher;
mod errors;
mod exclusion;
mod flattening;
mod flow;
mod job;
mod operators;
mod scope;
mod select;
mod shaping;
mod shared;
mod sharing;
mod state;
mod suspending;
mod sync;
mod task;
mod terminal;
mod testing;
mod timing;
mod transforms;

pub use builders::{Emit, Emitter, FlowBlock, FlowBlockRun, FlowOf, FlowOfRun, flow, flow_of};
pub use channel::{
    Capacity, ChannelFlow, ChannelFlowRun, ProduceIn, Producer, Receiver, RecvFuture, SendError,
    SendFuture, Sender, TryRecvError, TrySendError, callback_flow, channel, channel_flow, produce,
};
pub use clock::{Clock, Delay, SystemClock, TimedOut, WithTimeout, delay, with_timeout};
pub use combining::{
    Combine3, Combine3Run, Combine4, Combine4Run, Combine5, Combine5Run, CombineAll, CombineAllRun,
    Merge, MergeRun, Zip, ZipRun, combine_all, combine3, combine4, combine5, merge,
};
pub use dispatcher::{
    ConfinedDispatcher, Dispatch, Dispatcher, Dispatchers, IO_POOL_MIN_THREADS, Runnable,
};
pub use errors::{Catch, CatchRun, RetryRun, RetryWhen};
pub use exclusion::{Mutex, MutexGuard, Permit, Semaphore};
pub use flattening::{FlatMap, FlatMapRun, Flatten};
pub use flow::{BoxFlow, Flow, FlowExt, LocalBoxFlow, SendFlow};
pub use futures_core::Stream;
pub use job::{Job, JobOutcome, Join};
pub use operators::{
    Buffered, BufferedRun, ChangeKey, Combine, CombineRun, Debounce, DebounceRun, DistinctRun,
    DistinctUntilChanged, FLOW_ON_BUFFER, Filter, FilterRun, Map, MapRun, OnCompletion,
    OnCompletionRun, OnEach, OnEachRun, OnStart, StartWith, StartWithRun, Take, TakeRun,
    WholeValue,
};
pub use scope::{
    AwaitAll, BoxFuture, ChildFailed, CoroutineScope, Deferred, MainScope, Scope, ScopeHandle,
    Spawn, TaskFailed, WithContext, await_all, coroutine_scope, join_all, supervisor_scope,
    with_context,
};
pub use select::{Either, Select, SelectAll, YieldNow, select, select_all, yield_now};
pub use shaping::{
    Chunked, ChunkedRun, EmitterAction, OnEmpty, OnEmptyRun, RunningReduce, RunningReduceRun,
    WithIndex, WithIndexRun,
};
pub use shared::{
    BufferOverflow, EmitShared, MutableSharedFlow, OnSubscription, OnSubscriptionRun, SharedFlow,
    SharedRun,
};
pub use sharing::{
    CustomSharing, FirstState, SHARE_IN_BUFFER, SharingCommand, SharingStarted, SharingTask,
    StateInFirst,
};
pub use state::{MutableStateFlow, StateFlow, StateRun};
pub use suspending::{
    CollectAsync, CollectLatest, Emitting, FilterMapping, Filtering, Finish, InOrder, Inspecting,
    LatestOnly, Mapping, Order, Passing, Step, Suspending, SuspendingRun, Transforming,
    TransformingWhile, Verdict,
};
pub use terminal::{Collect, Count, First, Fold, Last, Reduce, Single, ToVec};
pub use testing::{Stalled, TestScheduler, TestScope, Turbine, run_test};
pub use timing::{Sample, SampleRun, Timeout, TimeoutRun};
pub use transforms::{
    FilterMap, FilterMapRun, Scan, ScanRun, Skip, SkipRun, Skipping, Taking, While, WhileRun,
};
