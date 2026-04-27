# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file tracks current forward work and marks boxes closed only after the stated verification gate.

## Current State

- Slot Table V2 is the active implementation under `crates/cranpose-core/src/slot/*`.
- Completed cleanup and review-fix history is in git through `0bbe9cd6`.
- The first refactor slice is closed below with its validation gate.
- Last full local verification on 2026-04-26 was green:
  `cargo fmt`, `cargo test > 1.tmp 2>&1`,
  `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`,
  Android `:app:assembleRelease`, `apps/desktop-demo/build-web.sh`,
  and `CRANPOSE_BUILD_JOBS=2 ./run_robot_test.sh --sequential`.
- Latest local verification on 2026-04-27 was green with serialized execution:
  `env CRANPOSE_USE_SCCACHE=0 ./verify_slot_table.sh` after capping Cargo,
  Rust test, Gradle, and robot execution to one worker/thread.
- Slot Table V2 perf comparison on the current tree at base commit `077b0559`
  also completed with `env CRANPOSE_USE_SCCACHE=0 CRANPOSE_BUILD_JOBS=1
  CARGO_BUILD_JOBS=1 ./perf_slot_table_v2.sh --baseline
  slot-v2-refactor-base`; settings: all benchmarks, CPU set `0`, warmup
  `1s`, measurement `5s`, sample size `30`. The only reported regression was
  `slot_table_v2_tab_switch_16_payload_groups` at about `+2.8%`, below the
  documented `5%` tab-switch budget; larger tab-switch, subcompose, lazy-list,
  and keyed benchmarks were within noise or improved.
- The next work is refactoring for maintainability only: same preorder `Vec`
  backend, same semantics, same tests, no key/retention/node-lifecycle rewrite.

## Refactor Guardrails

- Do not switch to a linked, arena, or alternate group-storage backend without satisfying `docs/slot_table_v2_link_backend_decision.md`.
- Do not change key semantics, `GroupStartKind`, retention ownership, node lifecycle, source-location key hashing, scope routing, or sibling-index threshold without a dedicated measured change.
- Do not add feature flags or half-states. Each item must leave the repo in one clean implementation.
- For each code item, run at minimum `cargo fmt`, `cargo test -p cranpose-core slot::`, `cargo test > 1.tmp 2>&1`, and `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`.
- For production slot-code refactors, run `./verify_slot_table.sh`. For structural hot-path refactors, also run the Slot Table V2 perf baseline comparison documented in `docs/slot_table_v2_performance_baselines.md`.

## Code Review Findings 2026-04-27

Scope reviewed: `crates/cranpose-core/src/slot/*`, `crates/cranpose-core/src/slot_storage.rs`, `crates/cranpose-core/src/retention.rs`, and the composer integration points that call slot storage.

### P0 - Remove Public Slot Internals That Are Not User API

- [x] Make `slot_storage.rs` crate-private API or move the handle types under `slot::types`, then stop re-exporting storage operation structs from `crates/cranpose-core/src/lib.rs`.
  Evidence: `lib.rs` publicly exports `BeginGroupInput`, `GroupAnchor`, `GroupId`, `GroupKey`, `GroupStart`, `GroupStartKind`, `NodeRecordResult`, and `ValueSlotId`, but these are composer/storage plumbing rather than user-facing APIs. `GroupKey` cannot be constructed externally because its constructor and fields are crate-private, so the public export is especially misleading.
  Files: `crates/cranpose-core/src/lib.rs`, `crates/cranpose-core/src/slot_storage.rs`.

- [x] Delete `GroupAnchor` or replace it with `AnchorId` at the single structural field that uses it.
  Evidence: `GroupAnchor` is only a type alias in `slot_storage.rs` and is only used by `GroupStart.anchor`; it adds a second name for the same concept without adding type safety.
  Files: `crates/cranpose-core/src/slot_storage.rs`.

- [x] Keep detailed `GroupStartKind::{Inserted, Reused, Moved}` out of the public surface unless production code needs those variants.
  Evidence: production composer logic only branches on `GroupStartKind::Restored`; `Inserted`, `Reused`, and `Moved` are asserted by slot tests and are useful as internal diagnostics, not public API.
  Files: `crates/cranpose-core/src/composer.rs`, `crates/cranpose-core/src/slot/writer/group.rs`, `crates/cranpose-core/src/slot_storage.rs`.

### P0 - Remove State That Is Not Connected To Runtime Semantics

- [x] Remove `NodeLifecycle::Disposed` and `DetachedSubtree::mark_nodes_disposed()` unless a real live-storage invariant starts observing disposed node records.
  Evidence: disposed subtrees are consumed immediately by `SlotLifecycleCoordinator::queue_subtree_disposal`; node records are not retained after that path. The `Disposed` variant is used to mark a subtree immediately before consuming it, and tests can validate retained lifecycle mismatch with `Active` versus `RetainedDetached` without a fake disposed state.
  Files: `crates/cranpose-core/src/slot/types.rs`, `crates/cranpose-core/src/slot/lifecycle.rs`, `crates/cranpose-core/src/slot/tests/retention.rs`.

- [x] Remove or make real use of `DetachedSubtree::generation`.
  Evidence: the generation is allocated on every detach and asserted as non-zero in a debug-only composer assertion, but it is not part of retention identity, stale subtree rejection, ordering, or diagnostics exposed to callers.
  Files: `crates/cranpose-core/src/slot/detach.rs`, `crates/cranpose-core/src/slot/types.rs`, `crates/cranpose-core/src/composer.rs`.

### P1 - Move Test-Only Convenience Out Of Production Modules

- [x] Move `SlotWriteSession::value_slot`, `SlotWriteSession::record_node`, `SlotWriteSession::nodes_in_current_group`, and `SlotTable::collect_subtree_node_ids` into the slot test harness or replace tests with the production methods.
  Evidence: these methods are `#[cfg(test)]`, called only from slot tests, and live beside the real writer API. The production methods are already `value_slot_with_kind`, `record_node_with_parent`, and `collect_subtree_root_node_ids`.
  Files: `crates/cranpose-core/src/slot/writer/payload.rs`, `crates/cranpose-core/src/slot/writer/nodes.rs`, `crates/cranpose-core/src/slot/tests/mod.rs`.

- [x] Move `AnchorRegistry::contains_active` and `AnchorRegistry::invalidate` into test-only helper code or express those tests through validated table operations.
  Evidence: both are `#[cfg(test)]` helpers on the production registry. `invalidate` mutates registry internals directly for corruption tests, which is valid test setup but should not live as a production-module method.
  Files: `crates/cranpose-core/src/slot/anchors.rs`, `crates/cranpose-core/src/slot/tests/validation.rs`.

### P1 - Collapse Duplicated Data-Structure Code

- [x] Replace the duplicated `PayloadRange` and `NodeRange` implementations with one typed range primitive.
  Evidence: both structs implement the same `new`, `from_start_len`, `len`, `is_empty`, `subrange`, and `as_range` behavior with only the panic label differing.
  Files: `crates/cranpose-core/src/slot/ranges.rs`.

- [x] Collapse `GroupPayloadRange` and `GroupNodeRange` around the same typed range primitive, preserving `start_offset` only where payload-location refresh needs it.
  Evidence: both structs wrap a group index plus a subrange. The payload version carries `start_offset`; the node version does not, so the common abstraction should model the shared shape and keep payload-specific refresh data outside the generic core.
  Files: `crates/cranpose-core/src/slot/ranges.rs`, `crates/cranpose-core/src/slot/payload.rs`, `crates/cranpose-core/src/slot/nodes.rs`.

- [ ] Reduce payload/node segment mutation duplication after the typed range cleanup.
  Evidence: `payload.rs` and `nodes.rs` both implement group segment start/len/range access, tail removal, subtree extraction, and subtree restore around the generic `GroupSegment` helpers. The abstraction exists but the table operations are still copy-shaped.
  Files: `crates/cranpose-core/src/slot/payload.rs`, `crates/cranpose-core/src/slot/nodes.rs`, `crates/cranpose-core/src/slot/segments.rs`.

### P1 - Simplify Verbose Validation Plumbing

- [ ] Replace the paired active/detached `SlotInvariantError` variants with one error payload that carries a slot-tree context.
  Evidence: `SlotInvariantError` duplicates active and detached variants for parent, depth, subtree len, payload start, payload range, payload count, payload owner, node start, node range, node count, node owner, and duplicate node id. `SlotTreeKind` then repeats a match for each error constructor. This is correct but too verbose and makes new invariants expensive to add.
  Files: `crates/cranpose-core/src/slot/validate/errors.rs`, `crates/cranpose-core/src/slot/validate/groups.rs`.

- [ ] Tighten payload-location validation diagnostics so reverse-registry failures report the actual stale record or use a distinct error variant.
  Evidence: `validate_payload_locations` compares the registry key against the payload record but can construct `PayloadLocationMismatch` with `actual: Some((owner, payload_index))`, the same tuple shape as the expected registry location, even when the record mismatch is the real problem.
  Files: `crates/cranpose-core/src/slot/validate/payloads.rs`.

### P1 - Stop Every-Pass Namespace Scans

- [ ] Gate anchor and payload namespace compaction behind explicit sparse/free counters instead of scanning active and retained storage after every pass.
  Evidence: `SlotsHost::complete_pass_cleanup` calls namespace compaction every pass. `ComposerRuntimeState::compact_table_namespaces_for_host` then runs both anchor and payload compaction, and each compaction computes retained counts and max ids before returning early. This is connected production code and should be event-driven by detach/dispose/free-list pressure, not a full scan on stable frames.
  Files: `crates/cranpose-core/src/lib.rs`, `crates/cranpose-core/src/composer.rs`, `crates/cranpose-core/src/slot/anchors.rs`, `crates/cranpose-core/src/slot/payload.rs`.

### P2 - Make Internal Corruption Fail Locally In Release Builds

- [ ] Replace release-wrap casts after `debug_assert!` with checked arithmetic in structural counters and segment offsets.
  Evidence: `adjust_ancestor_group_spans`, `adjust_ancestor_node_counts`, `add_group_segment_len`, and `apply_group_segment_start_delta` only debug-assert non-negative results before casting to `u32`. If an internal invariant is violated in release, the value can wrap into a huge table span instead of failing at the mutation site.
  Files: `crates/cranpose-core/src/slot/table/mutation.rs`, `crates/cranpose-core/src/slot/segments.rs`.

- [ ] Add checked conversions where `usize` indexes and lengths are stored as `u32`.
  Evidence: `GroupId::new`, `insert_new_group`, payload/node segment start updates, and subtree spans cast indexes or lengths to `u32`. A clean slot table should have one checked conversion helper so overflow behavior is consistent and local.
  Files: `crates/cranpose-core/src/slot_storage.rs`, `crates/cranpose-core/src/slot/table/mutation.rs`, `crates/cranpose-core/src/slot/segments.rs`.

### P2 - Clean Up Debug Surface Ownership

- [ ] Split table-local debug data from host/composer-retention debug data.
  Evidence: `SlotTableDebugStats` contains retained subtree fields, but raw `SlotTable::debug_stats()` fills only table-local fields and leaves retained fields at default zero. `SlotsHost::debug_stats()` later patches retained fields from `ComposerRuntimeState`. That makes `SlotTable::debug_stats()` look complete when it is only a partial view.
  Files: `crates/cranpose-core/src/slot/debug.rs`, `crates/cranpose-core/src/slot/reader.rs`, `crates/cranpose-core/src/lib.rs`, `crates/cranpose-core/src/composer.rs`.

- [ ] Replace `DetachedSubtree::root_nodes()` plus `root_nodes_iter()` with one allocation-conscious API.
  Evidence: `root_nodes_iter()` allocates a `Vec` via `root_nodes()` and immediately converts it to an iterator. Retention and disposal paths call these helpers on inactive subtrees; they should either return a borrowed cached root-node list or write roots into a caller-provided buffer.
  Files: `crates/cranpose-core/src/slot/types.rs`, `crates/cranpose-core/src/slot/detach.rs`, `crates/cranpose-core/src/composer.rs`.

## Suggested Execution Order

1. Public API cleanup: remove public re-exports and `GroupAnchor`.
2. Runtime semantic cleanup: delete unobserved disposed lifecycle state and decide the fate of detached generation.
3. Test helper cleanup: move `#[cfg(test)]` slot writer helpers into `slot/tests/mod.rs`.
4. Range and segment refactor: introduce a typed range primitive, then reduce payload/node duplication.
5. Validation refactor: collapse active/detached error duplication and fix payload-location reverse diagnostics.
6. Performance cleanup: make namespace compaction event-driven and measure with `./perf_slot_table_v2.sh --stability-check`.
7. Arithmetic hardening: centralize checked `usize`/`u32` conversions and release-mode structural counter checks.
