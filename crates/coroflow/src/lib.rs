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
//! | `withContext`, `delay`, `withTimeoutOrNull` | [`with_context`], [`delay`], [`with_timeout`] |
//! | `Flow`, `flow { }`, `flowOf` | [`Flow`], [`flow`], [`flow_of`] |
//! | `suspend fun` in an interface | a method returning [`BoxFuture`] |
//! | `map`, `filter`, `mapNotNull`, `scan`, `drop`, `debounce`, `zip`, `combine`, `flowOn` | [`FlowExt`] |
//! | `flatMapLatest`, `flatMapConcat`, `flatMapMerge`, `catch`, `retry`, `retryWhen` | [`FlowExt`] |
//! | `merge(a, b)`, `combine(a, b, c)` | [`merge`], [`combine3`] |
//! | `MutableStateFlow`, `MutableSharedFlow` | [`MutableStateFlow`], [`MutableSharedFlow`] |
//! | `stateIn(scope, WhileSubscribed(5000), x)`, `shareIn` | [`FlowExt::state_in`], [`FlowExt::share_in`], [`SharingStarted`] |
//! | `runTest`, `advanceTimeBy` | [`TestScheduler`] |
//! | Turbine's `flow.test { awaitItem() }` | [`Turbine`] |
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

mod builders;
mod clock;
mod combining;
mod dispatcher;
mod errors;
mod flattening;
mod flow;
mod job;
mod operators;
mod scope;
mod sharing;
mod state;
mod sync;
mod task;
mod terminal;
mod testing;
mod transforms;

pub use builders::{Emit, Emitter, FlowBlock, FlowBlockRun, FlowOf, FlowOfRun, flow, flow_of};
pub use clock::{Clock, Delay, SystemClock, TimedOut, WithTimeout, delay, with_timeout};
pub use combining::{Combine3, Combine3Run, Merge, MergeRun, Zip, ZipRun, combine3, merge};
pub use dispatcher::{
    ConfinedDispatcher, Dispatch, Dispatcher, Dispatchers, IO_POOL_MIN_THREADS, Runnable,
};
pub use errors::{Catch, CatchRun, RetryRun, RetryWhen};
pub use flattening::{FlatMap, FlatMapRun};
pub use flow::{BoxFlow, Flow, FlowExt, LocalBoxFlow, SendFlow};
pub use futures_core::Stream;
pub use job::{Job, JobOutcome, Join};
pub use operators::{
    Combine, CombineRun, Debounce, DebounceRun, DistinctRun, DistinctUntilChanged, FLOW_ON_BUFFER,
    Filter, FilterRun, FlowOn, FlowOnRun, Map, MapRun, OnCompletion, OnCompletionRun, OnEach,
    OnEachRun, OnStart, StartWith, StartWithRun, Take, TakeRun,
};
pub use scope::{
    BoxFuture, CoroutineScope, Deferred, MainScope, Scope, ScopeHandle, Spawn, TaskFailed,
    WithContext, with_context,
};
pub use sharing::{SHARE_IN_BUFFER, SharingStarted, SharingTask};
pub use state::{MutableSharedFlow, MutableStateFlow, SharedFlow, SharedRun, StateFlow, StateRun};
pub use terminal::{Collect, First, ToVec};
pub use testing::{Stalled, TestScheduler, Turbine};
pub use transforms::{FilterMap, FilterMapRun, Scan, ScanRun, Skip, SkipRun};
