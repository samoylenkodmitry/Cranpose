# coroflow

`coroflow` provides structured coroutines, jobs, dispatchers, channels, and
Kotlin-style `Flow` operators for Rust. Native targets use thread pools and a
timer thread; browser builds use the page event loop. A custom executor
implements `Dispatch`.

Depend directly on `coroflow` for async tasks or streams outside Cranpose.
Use [`cranpose-coroflow`](https://docs.rs/cranpose-coroflow/latest/cranpose_coroflow/)
for view models and composition-aware collection. The feature set is empty.

## Collect a flow

`flow_of` creates a finite stream. `run_test` supplies virtual time and waits
for the test body and child tasks:

```rust
use coroflow::{FlowExt, flow_of, run_test};

let result = run_test(async |_| {
    flow_of(vec![1, 2, 3])
        .map(|value| value * 2)
        .to_vec()
        .await
});

assert_eq!(result, Ok(vec![2, 4, 6]));
```

## Kotlin API map

| Kotlin | Rust API |
|---|---|
| `CoroutineScope`, `launch`, `async` | `CoroutineScope`, `Scope::launch`, `Scope::async_` |
| `Dispatchers.Default`, `Dispatchers.IO`, `Dispatchers.Main` | `Dispatchers::default_pool`, `Dispatchers::io`, `ConfinedDispatcher` |
| `delay`, `withContext`, `withTimeoutOrNull` | `delay`, `with_context`, `with_timeout` |
| `Flow`, `flow {}`, `flowOf` | `Flow`, `flow`, `flow_of` |
| `map`, `filter`, `combine`, `flatMapLatest` | `FlowExt`, `combine3`, `combine_all` |
| `Channel`, `channelFlow`, `callbackFlow` | `channel`, `channel_flow`, `callback_flow` |
| `MutableStateFlow`, `MutableSharedFlow` | `MutableStateFlow`, `MutableSharedFlow` |
| `map`, `filter`, `mapNotNull`, `scan`, `drop`, `debounce`, `zip`, `combine`, `flowOn` | `FlowExt` |
| `flatMapLatest`, `flatMapConcat`, `flatMapMerge`, `catch`, `retry`, `retryWhen` | `FlowExt` |
| `sample`, `timeout`, `onEmpty`, `withIndex`, `distinctUntilChangedBy`, `runningReduce`, `chunked` | `FlowExt` |
| `launchIn`, `produceIn`, `fold`, `reduceOrNull`, `count`, `lastOrNull`, `singleOrNull` | `FlowExt` |
| Async flow transformations | `map_async`, `filter_async`, `filter_map_async`, `on_each_async`, `collect_async` |
| Multi-flow operations | `merge`, `combine3`, `combine4`, `combine5`, `combine_all` |
| Synchronization | `Mutex`, `Semaphore`, `channel` |
| `runTest` and Turbine | `run_test`, `TestScheduler`, `Turbine` |

Flow operators are structs. A chain stores operator state once per collection.
Rust cancels a coroutine when its future drops. Background scopes require
`Send` captures; main-thread scopes stay thread-confined.

## Native worker budgets

`Dispatchers::default_pool()` and `Dispatchers::io()` share lazy workers.
Each lane can run up to the device's available parallelism; their combined
worker limit is twice that value. Blocked I/O leaves capacity for CPU work.
Workers retire after 30 idle seconds and release their thread-local resources.
Getting a dispatcher without submitting work starts no worker threads.

Use `DispatcherPool::new(DispatcherPoolConfig { ..Default::default() })` for
an owned pair with different CPU/I/O limits or an idle timeout. Its `cpu()` and
`io()` handles keep the pool alive independently. `Dispatchers::single_thread`
starts its dedicated thread on first use and preserves that thread's identity
until the dispatcher is no longer used.

Coroutine wake-ups are never rejected for overload. The limits bound executing
steps and worker threads; queued steps and suspended futures still retain data.
Bound producer concurrency with structured scopes, channels or semaphores when
launching large batches. Cancellation drops a future on its dispatcher and
cannot interrupt an already-running blocking call. Browser dispatchers use the
page event loop, where blocking code remains unsuitable.

Cranpose's `withBlocking` executor has its own bounded queue and worker budget.
These coroutine limits do not change that executor or require a context
parameter in composables.

- [API reference on docs.rs](https://docs.rs/coroflow/latest/coroflow/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/coroflow)
