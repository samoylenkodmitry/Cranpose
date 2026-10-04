# cranpose-core

`cranpose-core` provides the composition runtime, reactive state, composition
locals, runtime scheduler support, and coroutine hooks used by Cranpose. Most
application code uses these services through the [`cranpose` facade](https://docs.rs/cranpose/latest/cranpose/).

Depend directly on `cranpose-core` when you build a host integration, a
framework crate, or a lower-level runtime tool. App code with ordinary UI needs
the facade and widget crates instead.

## State and runtime

The host creates a `Runtime` with a scheduler. The runtime gives state a place
to live and accepts snapshot commits. State changes mark subscribed scopes
for recomposition.

```rust
use cranpose_core::{
    DefaultScheduler, Runtime, mutableStateOf, run_in_mutable_snapshot, scheduler_ref,
};

let _runtime = Runtime::new(scheduler_ref(DefaultScheduler));
let count = mutableStateOf(0);

assert_eq!(count.value(), 0);
let applied = run_in_mutable_snapshot(|| count.set(1));
assert!(applied.is_ok());
assert_eq!(count.value(), 1);
```

Composition code uses `remember` for values tied to a composition position and
`CompositionLocalProvider` for scoped values. `ownedMutableStateOf` gives Rust
owners an explicit state lifetime.

## Blocking work

`withBlocking` runs synchronous work on a shared native executor and returns
`Result<T, BlockingError>`. The pool starts threads on demand up to the available
CPU count, allows four waiting jobs per CPU, and retires idle threads after
30 seconds. A full queue returns `Saturated` immediately; it never waits for
capacity on the UI thread. Applications decide whether to retry, replace stale
work, or surface an error. `BlockingExecutor` provides explicit thread, queue,
and idle limits for workloads that need a different budget.

Dropping a work future removes a waiting job and releases its captures. A running
closure cannot be interrupted. Use `withBlocking` inside a composition's
`rememberCoroutineScope().launch(...)` to cancel pending work when that scope
leaves. `launchBlocking` delivers `Result` to a UI callback and returns a
`Result<TaskHandle, BlockingError>` for admission; cancel the handle to cancel
pending work and delivery. It requires a live runtime and never falls back to
running a closure on the caller thread.

Shutting down an executor, or dropping its last owner, rejects new work and
resolves waiting tasks with `Shutdown`. Running work may finish. Unwinding
panics resolve as `Panicked`; a binary built with `panic = "abort"` still aborts.
Web calls return `Unsupported` without executing the blocking closure. Use
browser asynchronous APIs or an application-owned Web Worker on that target.

## Features

The default feature set is empty. `hot-reload` selects source-structure keys for
development hot reload. `std-hash` selects the standard library hasher.

## Runtime structure

The slot table stores active groups, payloads, and nodes. Retained inactive
branches use detached subtrees. See the [slot table invariants](https://github.com/samoylenkodmitry/cranpose/blob/main/docs/slot_table_invariants.md)
and the [current slot table design](https://github.com/samoylenkodmitry/cranpose/blob/main/docs/cranpose_slot_table_v2_design.md).

- [API reference on docs.rs](https://docs.rs/cranpose-core/latest/cranpose_core/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-core)
