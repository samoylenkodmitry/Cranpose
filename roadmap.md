# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file tracks the current forward work and marks boxes closed only after full verification.

## Current State

- Slot Table V2 is the active implementation under `crates/cranpose-core/src/slot/*`.
- The old `slot_table.rs` wrapper surface is removed.
- Strict audit found remaining drift between `docs/cranpose_slot_table_v2_design.md` and the current implementation; the checklists below track the required convergence work.
- Last recorded full local verification on 2026-04-23 was green:
  - `cargo fmt`
  - `cargo test`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - Android `:app:assembleRelease`
  - `apps/desktop-demo/build-web.sh`
  - `./run_robot_test.sh --sequential`

## Required Work

- [x] Extend `FinishGroupResult` to carry `subtree_nodes` plus skip state and feed composer/recompose skip handling from storage results instead of rescanning the slot table.
- [x] Expand `DetachedSubtree` to store explicit `root_nodes`, `scope_ids`, detached-anchor metadata, and subtree generation instead of reconstructing them from `groups` and `nodes` on demand.
- [x] Add semantic payload classification (`PayloadKind`) to `PayloadRecord` and wire remember/param/return/effect/internal payloads through it so debug and lifecycle paths stop treating every payload as the same untyped box.
- [x] Replace the remaining composer-side root discovery with stored root-node metadata so skip/retain/dispose paths consume root ids carried by `FinishGroupResult`, `NodeRecord`, and `DetachedSubtree` instead of walking the applier.
- [x] Rework writer frames to use parent-bounded table ranges instead of eagerly cloning `old_children` vectors on every `open_group_frame`; writer traversal now operates on direct-child table boundaries with explicit invariant checks and no per-group child-list copies.

- [ ] Reduce modifier slice collection churn in lazy-list scroll-reuse hot paths.
- [ ] Reduce layout-box and semantics allocation churn in lazy-list scroll-reuse hot paths.
- [ ] Add retained-memory instrumentation and anchor-capacity diagnostics that are cheap enough for regular debug investigation and strong enough for regression tests.
- [ ] Audit lazy-list and subcompose retention/reuse behavior under perf load and add specialized policy only if profiling proves the generic retained-subtree path is the bottleneck.

## Follow-Up Work

- [ ] Remove full-table `recompute_all_metadata()` from `restore_subtree()` and make restore update anchors, scopes, and spans incrementally; current restore cost scales with the entire table.
- [x] Deduplicate detached-node disposal between `Composer::dispose_detached_nodes` and `slot/detach.rs::dispose_detached_node_now` so node cleanup semantics live in one place.
- [x] Remove or hide test-only helpers from production slot types, starting with `DetachedSubtree::node_ids()`; `#[allow(dead_code)]` on production slot APIs is a smell.
- [x] Replace aggregated `slots_len`/`slots_cap` counters in `SlotTableDebugStats` and leak tooling with V2-native per-table counters (`group_count`, `payload_count`, `node_count`, and related capacities).
- [x] Replace slot-linear debug surfaces such as `debug_dump_all_slots()` with V2-native diagnostics by switching the repo to typed slot-debug entries instead of fake linear slot rows.
- [ ] Pack `GroupRecord` fields into denser arrays if profiling shows group-table bandwidth or cache pressure matters.
- [ ] Add retained-subtree LRU limits once a real memory-budget policy exists.
- [ ] Add collision-resistant debug/profile keys if diagnostics show location-key collisions in real workloads.
- [ ] Add allocator-backed tables for `no_std` only if that target becomes real again.

## Next Execution Order

1. Create a checkpoint commit for the verified writer-frame slice before changing `restore_subtree()`; the tree is green and the next step rewrites core slot restore invariants.
2. Replace `restore_subtree()` global `recompute_all_metadata()` with incremental updates for active anchor indices, scope index entries, ancestor subtree spans, payload starts, node starts, and payload-location bookkeeping.
3. Add focused restore regression tests that prove retained subtree restore stays exact across sibling inserts, sibling moves, and scope reactivation without any full-table metadata recompute.
4. Add retained-memory instrumentation and cheap anchor-capacity diagnostics so further retention and restore work can be profiled without ad hoc debugging.
5. Profile lazy-list modifier/layout/semantics churn after the restore rewrite and only add specialized reuse policy if measurements show the generic retained-subtree path is still the bottleneck.
