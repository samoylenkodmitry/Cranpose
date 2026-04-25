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

## Full Verification Gate

- [x] [S] `cargo fmt`
- [x] [M] `cargo test > 1.tmp 2>&1`
- [x] [M] `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`
- [x] [L] Android release build: `./gradlew :app:assembleRelease` in `apps/android-demo/android`
- [x] [M] Wasm build: `apps/desktop-demo/build-web.sh`
- [x] [L] Robot e2e: `./run_robot_test.sh --sequential`
- [x] [M] Read every verification log and fix every warning/failure before committing.
- [ ] [S] Keep `docs/cranpose_slot_table_v2_design.md` as the only active slot-table design specification.
- [ ] [S] Add `docs/slot_table_v2_invariants.md` as the short invariant checklist for active groups, payload/node ownership, anchors, retention, scope lookup, and sibling matching.
- [ ] [S] Add `verify_slot_table.sh` that runs the full verification gate and writes/readable logs for every step.
- [ ] [S] Update user-facing crate docs to say Slot Table V2 is active and gap-table semantics are historical only.
- [ ] [S] Document the baseline process using `./perf_slot_table_v2.sh --save-baseline`, `--baseline`, and `--stability-check` in `docs/slot_table_v2_performance_baselines.md`.

## Slot Table Cleanup Backlog

- [x] [M] Remove dead production fields from `FinishGroupResult`; keep only detached children, direct node removals, skipped-root nodes, and skipped state.
- [x] [L] Collapse the half-wired `SlotStorage` trait boundary; either make it the real composer storage API or remove it and its test-only surface.
- [x] [XL] Replace duplicated detached-subtree metadata with one canonical detached tree representation; derive root key, root scope, root nodes, scope ids, and anchor lists from the records.
- [x] [S] Track removed payload count in `SlotWriteSessionState` so payload-heavy removals trigger slot-table compaction.
- [x] [XL] Factor duplicated payload/node segmented-storage mechanics into one reusable internal primitive.
- [x] [L] Deduplicate active-table and detached-subtree validation through one preorder/span validator over active and detached views.
- [x] [M] Move group-close teardown out of duplicated composer/recompose guards into one helper.
- [x] [M] Delete or gate remaining test-only/unfinished APIs: `AnchorRegistry::contains_active`, `AnchorRegistry::invalidate`, ignored lifecycle/mode/table parameters, and remaining production `allow(dead_code)` shims.
- [x] [M] Make `ValueSlotId` generation real or remove the unused generation field.
- [x] [S] Clean small slot-table garbage: over-reserved drop capacity in `take_all_drops`, single-variant `DeferredDrop`, and no-op `kind.label()` disposal.
