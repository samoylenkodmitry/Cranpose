# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file lists only unfinished work. Completed rewrite items were intentionally removed so this stays usable.

## Current State

- Slot Table V2 is the active implementation under `crates/cranpose-core/src/slot/*`.
- The old `slot_table.rs` wrapper surface is removed.
- Full local verification was green on the current tree:
  - `cargo test`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - Android `:app:assembleRelease`
  - `apps/desktop-demo/build-web.sh`
  - `./run_robot_test.sh --sequential`

## Remaining Required Work

- [ ] Keep `docs/cranpose_slot_table_v2_design.md` and the implementation aligned after every slot/composer change; do not let `roadmap.md` become a second spec again.
- [ ] Build repeatable performance baselines for keyed reorder, tab switching, subcompose scrolling, and lazy-list scroll reuse.
- [ ] Reduce unnecessary `Vec` cloning in slot-table and retention hot paths.
- [ ] Profile subtree insert, move, and detach costs; if `Vec::splice` remains hot after measurement, replace subtree moves with a chunked sequence without changing semantics.
- [ ] Add retained-memory instrumentation and anchor-capacity diagnostics that are cheap enough for regular debug investigation and strong enough for regression tests.
- [ ] Audit lazy-list and subcompose retention/reuse behavior under perf load and add specialized policy only if profiling proves the generic retained-subtree path is the bottleneck.

## Optional Follow-Up Work

- [ ] Pack `GroupRecord` fields into denser arrays if profiling shows group-table bandwidth or cache pressure matters.
- [ ] Add retained-subtree LRU limits once a real memory-budget policy exists.
- [ ] Add collision-resistant debug/profile keys if diagnostics show location-key collisions in real workloads.
- [ ] Add allocator-backed tables for `no_std` only if that target becomes real again.
