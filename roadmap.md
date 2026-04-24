# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file tracks the current forward work and marks boxes closed only after full verification.

## Current State

- Slot Table V2 is the active implementation under `crates/cranpose-core/src/slot/*`.
- The old `slot_table.rs` wrapper surface is removed.
- Strict audit convergence work against `docs/cranpose_slot_table_v2_design.md` is complete; the remaining unchecked items are conditional follow-ups that require profiling or a concrete target policy before implementation.
- Last recorded full local verification on 2026-04-24 was green:
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

- [x] Reduce modifier slice collection churn in lazy-list scroll-reuse hot paths.
- [x] Reduce layout-box and semantics allocation churn in lazy-list scroll-reuse hot paths.
- [x] Add retained-memory instrumentation and anchor-capacity diagnostics that are cheap enough for regular debug investigation and strong enough for regression tests; `SlotTableDebugStats` now reports retained subtree counts/heap plus active/detached/invalidated/free anchor breakdown, and the coverage exercises both raw slot tables and composition-owned retention.
- [x] Audit lazy-list and subcompose retention/reuse behavior under perf load and add specialized policy only if profiling proves the generic retained-subtree path is the bottleneck; perf instrumentation showed no retained-subtree bottleneck, so no specialized policy was added.

## Slot Table Review Mitigation

- [x] Make retained-subtree insertion reject or explicitly dispose displaced retained groups instead of silently overwriting a matching `RetainKey`.
- [x] Make `SlotTable::restore_subtree` reject detached subtrees whose root key does not match the requested `GroupKey`.
- [x] Make duplicate explicit sibling keys fail during writer traversal or always-on debug validation, not only when slot diagnostics are enabled manually.
- [x] Replace type-name based `PayloadKind` inference with explicit payload-kind ownership at the call site.
- [x] Add the lazy per-writer-frame sibling index described by `docs/cranpose_slot_table_v2_design.md` so large keyed sibling ranges do not rebuild transient maps per lookup.
- [x] Remove or rewrite stale duplicate slot-table design documentation so `docs/cranpose_slot_table_v2_design.md` remains the single active slot-table specification.

## Full Verification Gate

- [x] `cargo fmt`
- [x] `cargo test > 1.tmp 2>&1`
- [x] `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`
- [x] Android release build: `./gradlew :app:assembleRelease` in `apps/android-demo/android`
- [x] Wasm build: `apps/desktop-demo/build-web.sh`
- [x] Robot e2e: `./run_robot_test.sh --sequential`
- [x] Read every verification log and fix every warning/failure before committing.

## Follow-Up Work

- [x] Remove full-table `recompute_all_metadata()` from `restore_subtree()` and make restore update anchors, scopes, and spans incrementally; restore now updates active anchors, scope entries, ancestor spans, and payload locations incrementally.
- [x] Deduplicate detached-node disposal between `Composer::dispose_detached_nodes` and `slot/detach.rs::dispose_detached_node_now` so node cleanup semantics live in one place.
- [x] Remove or hide test-only helpers from production slot types, starting with `DetachedSubtree::node_ids()`; `#[allow(dead_code)]` on production slot APIs is a smell.
- [x] Replace aggregated `slots_len`/`slots_cap` counters in `SlotTableDebugStats` and leak tooling with V2-native per-table counters (`group_count`, `payload_count`, `node_count`, and related capacities).
- [x] Replace slot-linear debug surfaces such as `debug_dump_all_slots()` with V2-native diagnostics by switching the repo to typed slot-debug entries instead of fake linear slot rows.
- [ ] Pack `GroupRecord` fields into denser arrays if profiling shows group-table bandwidth or cache pressure matters.
- [ ] Add retained-subtree LRU limits once a real memory-budget policy exists.
- [ ] Add collision-resistant debug/profile keys if diagnostics show location-key collisions in real workloads.
- [ ] Add allocator-backed tables for `no_std` only if that target becomes real again.

## Production Hardening Roadmap

The execution order is correctness first, then documentation and repeatable gates, then performance evidence, then targeted optimization. A LinkBuffer/arena backend remains a conditional prototype only after the current V2 implementation has model-test coverage and measured structural-edit bottlenecks.

### Phase 0 - Freeze Current Truth

- [x] Keep `docs/cranpose_slot_table_v2_design.md` as the only active slot-table design specification.
- [x] Add `docs/slot_table_v2_invariants.md` as the short invariant checklist for active groups, payload/node ownership, anchors, retention, scope lookup, and sibling matching.
- [x] Add `verify_slot_table.sh` that runs the full verification gate and writes/readable logs for every step.
- [x] Update user-facing crate docs to say Slot Table V2 is active and gap-table semantics are historical only.

### Phase 1 - Strengthen Validation

- [x] Add retained-state validation that checks retained keys, detached anchors, retained scopes, retained node lifecycle, and detached root parentage.
- [x] Add detached-subtree validation for preorder, depths, payload owners, node owners, root nodes, and anchor locality.
- [x] Add a composition-level debug validation helper and call it from integration tests.
- [x] Add negative validation tests for retained active anchors, active scope-index leakage, payload owner leakage, disposed retained nodes, and duplicate retained keys.

### Phase 2 - Add Model And Property Tests

- [x] Add a deterministic model/property-test harness for Slot Table V2 render-frame scenarios.
- [x] Add a reference model for active roots, retained groups, payloads, nodes, scopes, and remembered values.
- [x] Generate complete render-frame scripts for conditionals, keyed moves, tab retention, remembered values, invalidation, and skip paths.
- [x] Add core properties for keyed identity, unkeyed positional identity, dispose/reset, retain/restore, nested detach/restore, inactive retained invalidation, skip metadata, stale payload alias prevention, active/retained anchor separation, and random frame validation.
- [x] Make property failures print a reproducible seed, compact scenario script, active debug snapshot, retained-subtree summary, and failed invariant.

### Phase 3 - Complete Behavior Integration Tests

- [x] Audit or add remember survival/reset tests for recomposition, default conditional disposal, and retained restoration.
- [x] Audit or add tab-retention tests covering preserved state with retention and disposal without retention.
- [x] Audit or add keyed and unkeyed list identity tests.
- [x] Audit or add active/inactive scope invalidation tests.
- [ ] Audit or add `DisposableEffect` cleanup tests for dispose versus retain.
- [ ] Audit or add retained/disposed node lifecycle tests.
- [ ] Audit or add subcompose and lazy-list slot reuse tests, including lazy-list jump alias prevention.

### Phase 4 - Lock Performance Baselines

- [ ] Expand `slot_table_v2` Criterion benchmarks across keyed reverse sizes, keyed rotate, seeded shuffle, conditional toggle positions, tab payload sizes, and lazy scroll/jump modes.
- [ ] Add allocation and storage counters for modifier slices, layout boxes, semantics, group/payload/node counts and capacities, and retained subtree counts/bytes.
- [ ] Document the baseline process using `./perf_slot_table_v2.sh --save-baseline`, `--baseline`, and `--stability-check`.
- [ ] Document regression budgets for keyed reorder, tab switching, subcompose scrolling, lazy-list reuse, retained bytes, and anchor growth.

### Phase 5 - Optimize Current V2 Hot Paths

- [ ] Benchmark sibling-index thresholds of 4, 8, 16, 32, and 64 before changing the default.
- [ ] Instrument subtree moves with counts/spans for moved groups, payloads, nodes, payload-location rebuilds, and group-index refresh.
- [ ] Optimize proven `move_subtree` hot spots without changing semantics.
- [ ] Continue lazy-list allocation-churn reductions only where counters show real allocation pressure.
- [ ] Revisit `GroupRecord` field packing only if profiling shows group-table bandwidth or cache pressure matters.

### Phase 6 - Improve Lifecycle And Memory Policy

- [ ] Design `RetentionBudget` with max retained subtrees, retained bytes, and age limits.
- [ ] Add eviction policy choices for least-recently-restored, least-recently-detached, and largest-first retention.
- [ ] Add retained-state diagnostics for retained counts, estimated bytes, and eviction totals.
- [ ] Add memory plateau tests for repeated tab/list/subcompose retention.

### Phase 7 - Conditional LinkBuffer/Arena Prototype

- [ ] Define objective trigger criteria before prototyping any linked backend.
- [ ] Add a storage abstraction that preserves the current V2 semantic API and keeps preorder `Vec` storage as the default backend.
- [ ] Prototype linked group storage behind an explicit feature only after the trigger criteria are met.
- [ ] Run the full model/property and integration test suite against both backends.
- [ ] Ship the linked backend only if it proves large structural-edit wins without normal-case or memory regressions.

### Phase 8 - Release Hardening

- [ ] Add CI jobs for default features, alternate hash/internal features, property smoke, criterion smoke, wasm build, Android release, and robot e2e.
- [ ] Add stress commands for slot validation, high-case property tests, perf stability, and sequential robot tests.
- [ ] Add a release checklist covering stale docs, full verification, persisted property failures, perf baseline, regression budgets, retained-memory plateau, anchor growth, and panic classification.

## PR Sequence

- [ ] PR 1: documentation and verification gate.
- [ ] PR 2: retained-state validation.
- [ ] PR 3: property-test harness.
- [ ] PR 4: behavior integration tests.
- [ ] PR 5: performance baseline matrix.
- [ ] PR 6: measured current-V2 optimizations.
- [ ] PR 7: retention memory budget.
- [ ] PR 8: optional LinkBuffer/arena prototype.

## Next Execution Order

- [x] Complete the Slot Table Review Mitigation items in order, with the full verification gate and a separate commit after each completed item.
- [x] Profile lazy-list modifier/layout/semantics churn after review mitigation and only add specialized reuse policy if measurements show the generic retained-subtree path is still the bottleneck.
- [x] Execute Phase 0 production-hardening items first, with one verified commit per closed checklist item.
