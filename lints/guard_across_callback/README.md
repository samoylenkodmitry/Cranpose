# guard_across_callback

A dylint lint for the AGENTS.md rule that a `RefCell` borrow or a lock lasts a few lines and never spans a callback.

### What it does

Finds a call to a closure, a function pointer or a `dyn Fn` while a `RefCell` borrow (`Ref`, `RefMut`) or a lock guard (`MutexGuard`, `RwLockReadGuard`, `RwLockWriteGuard`, and `lock_api`'s) that a `let` holds is alive in the same block.

### Why is this bad?

The callback is code the caller does not see. When it reaches the same cell or lock, the borrow panics or the lock deadlocks, at run time, on the path that happens to re-enter.

### Known problems

Only `let`-bound guards and direct calls in their block count: a temporary guard inside one expression, and a closure handed to a named function that calls it, are not seen. A call that lends the guard's data to the callback (`f(&*borrow)`, or a value a `let` took from the guard) is the `with` accessor's purpose and is left alone.

### Example

```rust
fn notify(listeners: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    let listeners = listeners.borrow();
    on_change();
    drop(listeners);
}
```

Use instead:

```rust
fn notify(listeners: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    let count = listeners.borrow().len();
    on_change();
}
```

### Running it

`just dylint` installs cargo-dylint and the nightly's compiler components when they are missing, runs the lint over the workspace and compares its sites with `scripts/ci/dylint_budget.toml`. The library builds with the repo's pinned nightly (`rust-toolchain-nightly.toml`); `rust-toolchain.toml` here names the same date. `cargo +nightly-2026-10-03 test` in this directory runs the UI test in `ui/`.
