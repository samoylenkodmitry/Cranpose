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
- The next work is refactoring for maintainability only: same preorder `Vec`
  backend, same semantics, same tests, no key/retention/node-lifecycle rewrite.

## Refactor Guardrails

- Do not switch to a linked, arena, or alternate group-storage backend without satisfying `docs/slot_table_v2_link_backend_decision.md`.
- Do not change key semantics, `GroupStartKind`, retention ownership, node lifecycle, source-location key hashing, scope routing, or sibling-index threshold without a dedicated measured change.
- Do not add feature flags or half-states. Each item must leave the repo in one clean implementation.
- For each code item, run at minimum `cargo fmt`, `cargo test -p cranpose-core slot::`, `cargo test > 1.tmp 2>&1`, and `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`.
- For production slot-code refactors, run `./verify_slot_table.sh`. For structural hot-path refactors, also run the Slot Table V2 perf baseline comparison documented in `docs/slot_table_v2_performance_baselines.md`.

## Slot Table Refactor Roadmap

- [x] [M] Split `crates/cranpose-core/src/slot/tests.rs` into behavior-focused modules under `crates/cranpose-core/src/slot/tests/` without changing assertions or test behavior.
  Target modules: `basic.rs`, `payloads.rs`, `keyed_reorder.rs`, `detach_restore.rs`, `retention.rs`, `nodes.rs`, `validation.rs`, `writer_state.rs`, and `model.rs`.
  Acceptance: `cargo test -p cranpose-core slot::`, then `./verify_slot_table.sh`.

- [x] [S] Save a Slot Table V2 refactor baseline before production-code movement.
  Suggested commands: `git tag slot-v2-refactor-base` and `./perf_slot_table_v2.sh --save-baseline slot-v2-refactor-base`.
  Baseline saved at commit `fddc3a7c` with `CRANPOSE_BUILD_JOBS=2 ./perf_slot_table_v2.sh --save-baseline slot-v2-refactor-base`; settings: all benchmarks, CPU set `0`, warmup `1s`, measurement `5s`, sample size `30`.
  Acceptance: baseline artifacts exist and the command/settings are recorded in the item commit message or notes.

- [x] [L] Move writer session state out of `slot/table/session.rs` into writer-owned modules without changing behavior.
  Target modules: `slot/writer/state.rs`, `slot/writer/frames.rs`, `slot/writer/keys.rs`, and `slot/writer/siblings.rs`.
  Keep `SlotWriteSessionState`, `RootFrame`, `GroupFrame`, and `SiblingIndex` names unless a replacement removes real ambiguity.
  Acceptance run: `cargo test -p cranpose-core slot::writer`, `cargo test -p cranpose-core slot::`, then `CRANPOSE_BUILD_JOBS=2 ./verify_slot_table.sh`.

- [x] [L] Split `slot/writer.rs` by operation family while keeping `slot/writer/mod.rs` as the facade.
  Target modules: `group.rs`, `finish.rs`, `payload.rs`, `nodes.rs`, and `finalize.rs`.
  Do not change algorithms in this step.
  Acceptance run: `cargo test -p cranpose-core slot::`, then `CRANPOSE_BUILD_JOBS=2 ./verify_slot_table.sh`.

- [x] [L] Extract active child resolution from `begin_group`.
  Add an internal `ActiveChildResolution` flow where resolution inspects only active direct siblings and does not mutate storage; materialization performs only move/insert/reuse.
  Acceptance run: keyed reorder, duplicate-key, retained-restore, and model slot tests passed under `cargo test -p cranpose-core slot::`; `CRANPOSE_BUILD_JOBS=2 ./verify_slot_table.sh`.
  Perf note: CPU0 perf was rejected as contaminated by external load and thermal throttling. Same-CPU validation used `CRANPOSE_SLOT_TABLE_CPU_SET=29 CARGO_TARGET_DIR=/tmp/cranpose-slot-perf-target ./perf_slot_table_v2.sh --save-baseline slot-v2-refactor-base-cpu29` from tag `slot-v2-refactor-base`, then `CRANPOSE_SLOT_TABLE_CPU_SET=29 CARGO_TARGET_DIR=/tmp/cranpose-slot-perf-target ./perf_slot_table_v2.sh --baseline slot-v2-refactor-base-cpu29` from the current tree, with all benchmark families inside the documented budgets.

- [ ] [M] Encapsulate writer frame cursor mutation behind named methods.
  Replace direct writes to payload/node cursors, body-finished state, skip state, and parent child cursor advancement with methods on `GroupFrame` and `SlotWriteSessionState`.
  Acceptance: `cargo test -p cranpose-core slot::`, then `./verify_slot_table.sh`.

- [ ] [M] Add internal typed ranges for high-risk structural operations.
  Start with subtree, payload, and node ranges used by `move_subtree`, `detach_subtree`, `restore_subtree`, `remove_payload_range`, `remove_group_node_range`, and direct-child range APIs.
  Do not newtype every index at once.
  Acceptance: `cargo test -p cranpose-core slot::`, `./verify_slot_table.sh`, and `./perf_slot_table_v2.sh --baseline slot-v2-refactor-base`.

- [ ] [M] Narrow direct `SlotTable` field access after the writer split.
  Keep named APIs for sensitive operations such as active group lookup, direct-child lookup, subtree move/detach/restore, payload replacement, node recording, and range removal.
  Avoid low-value getter sprawl; only hide fields where direct mutation can violate invariants.
  Acceptance: `cargo test -p cranpose-core slot::`, then `./verify_slot_table.sh`.

- [ ] [L] Split `slot/validate.rs` into invariant-family modules while keeping `SlotInvariantError` stable.
  Target modules: `groups.rs`, `payloads.rs`, `nodes.rs`, `anchors.rs`, `scopes.rs`, `detached.rs`, and `writer.rs`.
  Do not weaken any invariant or change error semantics.
  Acceptance: validation-focused tests pass under `cargo test -p cranpose-core slot::`; run `./verify_slot_table.sh`.

- [ ] [S] Add debug-only structural tripwires after the refactor is complete.
  Candidate locations: after `move_subtree`, `detach_subtree`, `restore_subtree`, writer body finish in debug/test builds, and pass finalization.
  Keep full validation out of release hot paths.
  Acceptance: `cargo test -p cranpose-core slot::`, then `./verify_slot_table.sh`.
