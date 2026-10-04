#![doc = include_str!("../README.md")]

/// Kotlin's default number of inner flows [`FlowExt::flat_map_merge`] and
/// [`FlowExt::flatten_merge`] collect at once.
pub const DEFAULT_CONCURRENCY: usize = 16;

mod builders;
mod channel;
mod clock;
mod combining;
mod dispatcher;
mod dispatcher_pool;
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
pub use dispatcher::{ConfinedDispatcher, Dispatch, Dispatcher, Dispatchers, Runnable};
pub use dispatcher_pool::{DispatcherPool, DispatcherPoolConfig};
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
