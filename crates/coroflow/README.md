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

- [API reference on docs.rs](https://docs.rs/coroflow/latest/coroflow/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/coroflow)
