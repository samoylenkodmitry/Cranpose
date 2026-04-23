# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file tracks the current forward work and marks boxes closed only after full verification.

## Current State

- Slot Table V2 is the active implementation under `crates/cranpose-core/src/slot/*`.
- The old `slot_table.rs` wrapper surface is removed.
- The design docs and the implementation now agree on composer-owned runtime state, per-host scope resolution, and retained detach semantics.
- Full local verification is green on the current tree:
  - `cargo test`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - Android `:app:assembleRelease`
  - `apps/desktop-demo/build-web.sh`
  - `./run_robot_test.sh --sequential`

## Open Required Work

- [ ] Reduce modifier slice collection churn in lazy-list scroll-reuse hot paths.
- [ ] Reduce layout-box and semantics allocation churn in lazy-list scroll-reuse hot paths.
- [ ] Add retained-memory instrumentation and anchor-capacity diagnostics that are cheap enough for regular debug investigation and strong enough for regression tests.
- [ ] Audit lazy-list and subcompose retention/reuse behavior under perf load and add specialized policy only if profiling proves the generic retained-subtree path is the bottleneck.

## Open Follow-Up Work

- [ ] Pack `GroupRecord` fields into denser arrays if profiling shows group-table bandwidth or cache pressure matters.
- [ ] Add retained-subtree LRU limits once a real memory-budget policy exists.
- [ ] Add collision-resistant debug/profile keys if diagnostics show location-key collisions in real workloads.
- [ ] Add allocator-backed tables for `no_std` only if that target becomes real again.
