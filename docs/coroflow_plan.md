# Coroflow implementation record

`coroflow` provides coroutine scopes, jobs, dispatchers, flows, operators,
shared state flows and virtual-time test support. `cranpose-coroflow` connects those
APIs to Cranpose effects, lifecycle and view models. The implementation
checklist is complete; current APIs live in
[`coroflow`](../crates/coroflow/src/lib.rs) and
[`cranpose-coroflow`](../crates/cranpose-coroflow/src/lib.rs).

## API conventions

- Rust names use `snake_case`; `drop` maps to `skip` to avoid confusion with
  Rust's destruction term.
- Callbacks with suspension points use `async` closures. Captures follow Rust ownership
  rules, and operators clone the callback for each value when required.
- A failing flow emits `Result` values. Panics indicate bugs.
- Kotlin behavior guides the implementation, with the differences below.

## Cranpose integration

`cranpose-coroflow` supplies lifecycle-aware collection, coroutine scopes,
lifecycle effects, saved-state handles and scoped view model stores. The
navigation crate uses these stores for back-stack entries. The coroutine demo
shows the integrated APIs; see [`apps/coroflow-demo`](../apps/coroflow-demo).

## Deliberate differences

- Cancellation drops the coroutine future. `Drop` runs cleanup synchronously.
  The API omits `isActive`, `ensureActive` and `NonCancellable`.
- Rust infers `Send` from captured values.

## Recorded benchmark snapshot

Apple M5, Rust 1.98.1; Criterion medians:

| Work | coroflow | Comparison |
| --- | --- | --- |
| Launch a coroutine and wait | 188 ns | Tokio `current_thread`: 204 ns |
| Set state and read its change | 15.7 ns | Tokio `watch`: 70 ns |
| Broadcast one event to one collector | 24.2 ns | Tokio `broadcast`: 18.6 ns |
| 10,000 values through five operators | 9.4 µs | `futures` stream: 13.6 µs |

These numbers record one run; compare fresh measurements for current
performance. The shared-flow round trip recorded higher latency because each
emit and read searched for the slowest collector under the flow lock.
