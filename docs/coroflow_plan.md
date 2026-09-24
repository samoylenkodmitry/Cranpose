# Coroflow: Kotlin parity plan

What `coroflow` and `cranpose-coroflow` still need so that an app written
against `kotlinx.coroutines`, Flow and Jetpack Compose ports across line by line,
with the same behavior. Items are ticked as they land.

## API rules

- **Kotlin names in snake_case.** `flatMapLatest` is `flat_map_latest`,
  `stateIn` is `state_in`. The one exception is `drop`, which is `skip`,
  because `drop` means destruction in Rust.
- **Suspending lambdas are plain `async` closures.** Wherever Kotlin takes a
  `suspend` lambda, pass `async move |value| repository.load(value).await`.
  There is no `clone()` at the call site. The closure is cloned per value
  instead, so capture shared state as `Arc`/`Rc`, as a Kotlin reference would be.
- **Operators whose only point is suspending take the Kotlin name**:
  `transform`, `map_latest`, `collect_latest`. Operators that also have a
  cheaper synchronous form keep the Kotlin name for that form and add `_async`
  for the suspending one, as in `map` and `map_async`.
- **Errors are values.** A failing flow emits `Result` items. `catch`, `retry`
  and `timeout` work on those. A panic is a bug, not a flow error.
- **Behavior is Kotlin's.** Where the two differ, the difference is a bug here
  unless it is listed under "Deliberate differences" below.
- **Cost model.** An operator chain allocates once per collection and never per
  value. Every operator has a test. The behavior tests run on virtual time.

## A. Suspending lambdas

- [x] `transform(async move |value, emitter| ...)`: Kotlin's `transform`
- [x] `map_async`, `filter_async`, `filter_map_async` (`mapNotNull`) and `on_each_async`
- [x] `collect_async`: `collect` with a suspending action
- [x] `map_latest`, `transform_latest` and `collect_latest`
- [x] `transform_while`, plus `take_while` and `skip_while` (`dropWhile`)

## B. Operators and terminals

- [x] `sample`, `timeout` (emits `Err(TimedOut)` and completes), `on_empty`
- [x] `with_index`, `distinct_until_changed_by`, `running_reduce`, `chunked`
- [x] `flatten_concat` and `flatten_merge`
- [x] `Emitter::emit_all`, `launch_in(&scope)` and `produce_in(&scope)`
- [x] `combine_all(flows, transform)`, plus `combine4` and `combine5`
- [x] Terminals: `fold`, `reduce`, `count`, `last` and `single`

## C. Hot flows

- [x] `StateFlow`: `compare_and_set`, `update_and_get` and `get_and_update`
- [x] `subscription_count()` returns a `StateFlow<usize>` on both `StateFlow` and `SharedFlow`
- [x] `SharedFlow`: `replay_cache`, `reset_replay_cache` and `on_subscription`
- [x] A suspending `state_in_first(&scope)` that waits for the first value: Kotlin's `stateIn(scope)`
- [x] Custom `SharingStarted` strategies
- [x] A suspended `emit` that is cancelled withdraws its value, as in Kotlin
- [x] `share_in` suspends its upstream once its 64-value buffer is full, instead of dropping the oldest

## D. Jobs and dispatchers

- [x] `Job::invoke_on_completion` and `Job::cancel_and_join`; `Scope::children` (a coroutine's children are the ones launched in the scope it opens)
- [x] `join_all` and `await_all`
- [x] Lazy start: `launch_lazy` and `async_lazy`, then `start()`, `join()` or `.await`; `Deferred` dereferences to its `Job`
- [x] Failure handler: Kotlin's `CoroutineExceptionHandler`
- [x] `Dispatcher::limited_parallelism`, `Dispatchers::single_thread` and `Dispatchers::unconfined`

## E. Testing

- [x] `TestScheduler::advance_until_idle`
- [x] `run_test` with a `background_scope` that is cancelled when the test ends
- [x] Turbine: `await_item`, `await_complete`, `expect_no_events`, `skip_items` and `cancel_and_ignore_remaining_events`

## F. Cranpose

- [x] `collectAsState(initial)` and `collectAsStateWithLifecycle(initial)` for any `Flow`
- [x] `rememberCoroutineScope()` returning a coroflow `MainScope`
- [x] Coroflow code that runs inside Cranpose's own `LaunchedEffect`, `produceState` and
      UI tasks finds the main dispatcher, so `channel_flow`, `buffer` and `launch` work there
- [x] Lifecycle as a `StateFlow`, `flow_with_lifecycle` and `repeat_on_lifecycle`
- [x] `SavedStateHandle` for view models, backed by `rememberSaveable`
- [x] `flow`, `channel_flow` and `callback_flow` take plain async closures, so their captures need no cloning
- [x] The demo uses the new operators where they read better

## G. Verification

- [x] The coroflow demo runs on the Android emulator (`just android-coroflow`) and the iOS simulator (`just ios-run-coroflow`)
- [x] Benchmarks: `launch`, `emit`, and a five-operator chain, against `futures` and `tokio` (`cargo bench -p coroflow --bench comparisons`)

### Measured

Apple M5, Rust 1.98.1, criterion medians:

| Work | coroflow | Comparison |
| --- | --- | --- |
| Launch a coroutine and wait for it | 188 ns | tokio `current_thread`: 204 ns |
| Set a state and read the change | 15.7 ns | tokio `watch`: 70 ns |
| Broadcast an event to one collector | 24.2 ns | tokio `broadcast`: 18.6 ns |
| 10,000 values through five operators | 9.4 µs | `futures` stream: 13.6 µs |

The shared flow round trip is the one slower case: each emit and each read
finds the slowest collector again under the flow's lock.

## Deliberate differences

- Cancellation drops the coroutine's future, so there is no `isActive`,
  `ensureActive` or `NonCancellable`. Cleanup runs in `Drop` and cannot suspend.
- `Send` is inferred from what a flow or coroutine captures, not declared.
- View model stores, `NavHost` and screen-scoped view models are planned in
  [navigation_plan.md](navigation_plan.md).
