# guard_across_callback

A dylint lint for the AGENTS.md rule that a `RefCell` borrow or a lock lasts a few lines and never spans a callback.

### What it does

Finds code the caller does not see that runs while a `RefCell` borrow (`Ref`, `RefMut`) or a lock guard (`MutexGuard`, `RwLockReadGuard`, `RwLockWriteGuard`, and `lock_api`'s) that a `let` holds is alive in the same block. That code is one of these:

- a call to a closure, a function pointer or a `dyn Fn`
- a call to a function of the crate whose body reaches a callback, a `dyn` method or the drop of a value that holds a trait object (`Box<dyn Any>`, `Rc<dyn Fn()>`), directly or through other functions, closures and `Drop` impls of the crate
- a `drop` of a value whose drop reaches such code

### Why is this bad?

The callback is code the caller does not see. When it reaches the same cell or lock, the borrow panics or the lock deadlocks, at run time, on the path that happens to re-enter. A `Drop` impl added far away makes an old call re-enter, so the lint follows each call into its body.

### Known problems

Only `let`-bound guards and calls in their block count: a temporary guard inside one expression is not seen. The analysis stays inside one crate. A function of another crate counts as plain code, and only `dyn` methods of traits outside the standard library count as callbacks there. A drop inside a standard library function (`Vec::clear`) or at the end of a scope is not seen. A call that lends the guard's data to a closure (`f(&*borrow)`, or a value a `let` took from the guard) is the `with` accessor's purpose and is left alone. A named function that takes the guard's data still counts: its body can reach the cell another way.

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

`just dylint` installs cargo-dylint and the nightly's compiler components when they are missing, runs the UI test, runs the lint over the workspace and compares its sites with `scripts/ci/dylint_budget.toml`. The lint skips test crates: a test runs its re-entry, so the failure shows, and tests hold a global test lock for their whole body. The library builds with the repo's pinned nightly (`rust-toolchain-nightly.toml`); `rust-toolchain.toml` here names the same date. `cargo +nightly-2026-10-03 test` in this directory runs the UI test in `ui/`.
