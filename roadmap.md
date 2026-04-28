# Slot Table V2 Forward Roadmap

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This file tracks current forward work and marks boxes closed only after the stated verification gate.


## Refactor Guardrails

- Do not switch to a linked, arena, or alternate group-storage backend without satisfying `docs/slot_table_v2_link_backend_decision.md`.
- Do not change key semantics, `GroupStartKind`, retention ownership, node lifecycle, source-location key hashing, scope routing, or sibling-index threshold without a dedicated measured change.
- Do not add feature flags or half-states. Each item must leave the repo in one clean implementation.
- For each code item, run at minimum `cargo fmt`, `cargo test -p cranpose-core slot::`, `cargo test > 1.tmp 2>&1`, and `cargo clippy --workspace --all-targets -- -D warnings > 2.tmp 2>&1`.
- For production slot-code refactors, run `./verify_slot_table.sh`. For structural hot-path refactors, also run the Slot Table V2 perf baseline comparison documented in `docs/slot_table_v2_performance_baselines.md`.

Use this as the refactoring roadmap. The coding agent should copy it into `roadmap.md` or a dedicated `docs/slot_table_identity_refactor_roadmap.md`, then work strictly from top to bottom.

The architectural target is fixed: **Slot Table V2 stays; active groups remain preorder tables; inactive retained branches remain explicit detached subtrees; records may move, but identities must never be renamed by compaction.** The repo README and V2 design doc already identify Slot Table V2 as the active architecture, with active groups in preorder group/payload/node tables and retained inactive branches as explicit detached subtrees. ([GitHub][1])

Current `main` still has the exact problems this roadmap addresses: `ValueSlotId` is anchor-plus-generation, `GroupId` is still named like a stable id even though it is an active index handle, and `NodeRecordResult` is still a boolean result. ([GitHub][2]) Current anchor and payload compaction still remap active/retained ids instead of only compacting storage. ([GitHub][3])

The per-item gate below uses the repo’s own process: `verify_slot_table.sh` runs formatting, tests, clippy with warnings denied, Android release build, wasm build, and robot e2e; `stress_slot_table.sh` runs slot validation, generated-frame model stress, slot-table perf stability, and robot e2e. ([GitHub][4]) The repo’s `AGENTS.md` also requires clean architecture work, no half-migrated states, zero warnings, Android/wasm/robot validation, and self-review before accepting code. ([GitHub][5])

## Non-negotiable execution loop

For **every** checkbox below:

1. Work only on the first unchecked item.
2. Start with `git status --short`.
3. Add or update the regression test first inside the local worktree.
4. Implement the item completely; leave no half-state.
5. Run the full validation gate:

```bash
./verify_slot_table.sh
./stress_slot_table.sh
```

6. If anything fails, read the logs, fix the root cause, and rerun the full gate.
7. Self-review before committing:

```bash
git diff --check
git status --short
git diff --stat
git diff
```

8. Review specifically for: renamed identities, stale handles, hidden raw indices, duplicated logic, “temporary” comments, missing validation, warnings, unnecessary compatibility wrappers, and architecture shortcuts.
9. Mark the checkbox `[x]` only after validation is green and review is complete.
10. Commit the code, tests, docs, and checkbox update together.
11. Repeat until every checkbox is `[x]`.

---

# Slot Table Identity Refactor Roadmap

## Phase 0 — Put the roadmap under version control

* [x] Add this checklist to the repository roadmap.

  Done means: this exact roadmap exists in `roadmap.md` or `docs/slot_table_identity_refactor_roadmap.md`; the execution loop and validation gate are included; no code behavior changes are made.

  Commit message:

  ```text
  slot: add identity refactor roadmap
  ```

* [x] Add a small test-only identity inspection harness.

  Done means: tests can capture active group anchors, retained group anchors, active payload anchors, retained payload anchors, `ValueSlotId`s, scope ids, and slot debug stats without weakening production visibility. The helpers live under `#[cfg(test)]` or existing slot test modules. They do not introduce production-only dead code.

  Commit message:

  ```text
  slot: add identity inspection test harness
  ```

---

## Phase 1 — Lock the correct identity contract in tests

* [x] Add regression coverage proving active group anchors survive storage compaction.

  Done means: a test creates active groups, records their `AnchorId`s, triggers the storage/namespace compaction path currently used by the runtime, validates the table, and asserts the same anchors still identify the same groups.

  The implementation for this item must fix the code enough for this test to pass; no red test commit.

  Commit message:

  ```text
  slot: preserve active group anchors during compaction
  ```

* [x] Add regression coverage proving retained group anchors survive storage compaction.

  Done means: a test detaches and retains a subtree, records every retained group anchor, runs compaction with retention present, validates active and retained structures, restores the subtree, and proves the same anchors are restored.

  Commit message:

  ```text
  slot: preserve retained group anchors during compaction
  ```

* [x] Add regression coverage proving active `ValueSlotId`s survive storage compaction.

  Done means: a test obtains a `ValueSlotId`, reads the value, runs storage compaction, validates the table, and reads the same value through the same handle.

  Commit message:

  ```text
  slot: preserve active value slot identity during compaction
  ```

* [x] Add regression coverage proving retained payload anchors survive detach, retention, compaction, and restore.

  Done means: a retained subtree keeps remembered payload identity while inactive; compaction does not rename payload ids; restore reactivates the same payload anchors; stale handles fail only after semantic replacement or disposal, not after storage movement.

  Commit message:

  ```text
  slot: preserve retained value slot identity
  ```

* [x] Add regression coverage proving disposed identities are reused only with bumped generation.

  Done means: disposed group anchors and disposed payload anchors may reuse numeric ids only after generation bump; stale old handles fail cleanly; active or retained identities are never reused.

  Commit message:

  ```text
  slot: require generation bump on disposed identity reuse
  ```

---

## Phase 2 — Replace payload-location semantics with a real payload anchor registry

* [x] Introduce `PayloadAnchor` as the semantic payload identity.

  Done means: `PayloadAnchor` is a typed generational handle, not a naked `usize`; `PayloadRecord` stores `PayloadAnchor`; `ValueSlotId` stores `PayloadAnchor`; callers cannot confuse payload anchor ids with table indices.

  Required shape:

  ```rust
  #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
  pub(crate) struct PayloadAnchor {
      id: u32,
      generation: u32,
  }

  #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
  pub(crate) struct ValueSlotId {
      anchor: PayloadAnchor,
  }
  ```

  Commit message:

  ```text
  slot: introduce typed payload anchors
  ```

* [x] Add `PayloadAnchorRegistry`.

  Done means: `SlotTable` owns a registry that tracks payload anchors as `Active { owner, index }`, `Detached`, or `Invalidated`; the registry owns free ids and generation reuse; raw `next_payload_anchor` and raw `next_payload_generation` are removed from `SlotTable`.

  Required behavior:

  ```text
  insert payload       -> allocate stable PayloadAnchor
  reuse same slot/type -> keep same PayloadAnchor
  replace slot type    -> keep id, bump generation
  detach subtree       -> mark anchors Detached
  restore subtree      -> mark anchors Active
  dispose subtree      -> invalidate anchors and enqueue id for generation-bumped reuse
  compact storage      -> never rename anchors
  ```

  Commit message:

  ```text
  slot: add payload anchor registry
  ```

* [x] Replace `PayloadLocationRegistry` resolution with `PayloadAnchorRegistry` resolution.

  Done means: `read_value`, `read_value_mut`, and `write_value` resolve `ValueSlotId` through the payload anchor registry; detached and invalidated payload handles fail cleanly; validation checks every active payload record has exactly one active registry entry and every active registry entry points back to the correct record.

  Commit message:

  ```text
  slot: resolve value slots through payload anchors
  ```

* [x] Wire payload anchor lifecycle into detach, restore, and disposal.

  Done means: detaching payloads marks anchors detached; restoring retained payloads marks them active with new owner/index locations; disposing detached subtrees invalidates payload anchors; removing payload tails invalidates removed anchors; replacing payload identity bumps generation and invalidates stale `ValueSlotId`s.

  Commit message:

  ```text
  slot: wire payload anchor lifecycle
  ```

* [x] Delete payload namespace renumbering.

  Done means: `compact_payload_anchor_namespace` is removed or renamed to storage-only compaction; no code path rewrites `payload.anchor` for active or retained payloads; compaction may shrink maps/vectors/free lists only.

  Commit message:

  ```text
  slot: remove payload anchor renumbering
  ```

* [x] Strengthen payload validation.

  Done means: `validate()` verifies active payload anchors, detached payload anchors in retained subtrees, invalidated/free ids, generation mismatches, duplicate payload anchors, stale active locations, and retained payload identity consistency.

  Commit message:

  ```text
  slot: validate payload anchor registry
  ```

---

## Phase 3 — Make group anchors stable forever across storage compaction

* [x] Delete group anchor namespace renumbering.

  Done means: `compact_anchor_registry_storage` no longer remaps `AnchorId`s in active groups, retained groups, payload owners, node owners, parent anchors, scope anchors, or `RecomposeScope`s. The operation becomes storage-only cleanup: shrink sparse backing storage, compact free-list capacity, and keep identities unchanged.

  Commit message:

  ```text
  slot: remove group anchor renumbering
  ```

* [x] Rename group anchor compaction APIs to storage compaction names.

  Done means: names no longer imply namespace/id compaction. Use names like `compact_anchor_registry_storage`, `compact_payload_anchor_registry_storage`, or `compact_slot_backing_storage`. The codebase has no remaining “namespace compaction” path for live or retained identities.

  Commit message:

  ```text
  slot: rename compaction to storage cleanup
  ```

* [x] Strengthen group anchor validation.

  Done means: validation proves every active group has one active anchor entry, every retained group has detached anchor state, invalidated anchors are not referenced by active or retained records, parent anchors resolve correctly, and free ids cannot represent active or retained anchors.

  Commit message:

  ```text
  slot: validate stable group anchors
  ```

* [x] Add retained-anchor restore validation.

  Done means: restoring a retained subtree reactivates the same anchors; parent anchor changes only at the restored root; child parent anchors remain internally consistent; retained scopes regain the same group anchors.

  Commit message:

  ```text
  slot: validate retained anchor restore
  ```

---

## Phase 4 — Rename `GroupId` to the correct concept

* [ ] Rename `GroupId` to `ActiveGroupId`.

  Done means: all crate-internal APIs that currently use `GroupId` are renamed to `ActiveGroupId`; every call site makes it clear this is an active-table index-plus-generation handle, not stable identity.

  Required rule:

  ```text
  ActiveGroupId is transient.
  AnchorId is stable.
  RecomposeScope stores AnchorId.
  Retention stores AnchorId.
  DetachedSubtree stores AnchorId.
  ```

  Commit message:

  ```text
  slot: rename group id to active group id
  ```

* [ ] Audit and remove any stored `ActiveGroupId` outside active writer/reader operations.

  Done means: no scope, retention record, detached subtree, composer runtime state, debug-retained state, or node/payload owner stores `ActiveGroupId`. They store `AnchorId` or semantic ids only.

  Commit message:

  ```text
  slot: keep active group ids transient
  ```

* [ ] Add stale `ActiveGroupId` tests.

  Done means: tests prove an `ActiveGroupId` fails after generation mismatch, movement, removal, or disposal when it no longer points to the same active record; stable lookup is always done through `AnchorId`.

  Commit message:

  ```text
  slot: test transient active group ids
  ```

---

## Phase 5 — Replace raw structural mutation indices with typed cursors

* [ ] Introduce `ChildCursor`, `ActiveSubtreeRoot`, and `DetachedChild`.

  Done means: structural mutation entry points stop accepting naked `(usize, AnchorId, GroupKey, DetachedSubtree)` combinations where possible. The cursor carries parent anchor and child insertion boundary together.

  Required shape:

  ```rust
  #[derive(Copy, Clone, Debug, Eq, PartialEq)]
  pub(crate) struct ChildCursor {
      parent: AnchorId,
      index: usize,
  }

  #[derive(Copy, Clone, Debug, Eq, PartialEq)]
  pub(crate) struct ActiveSubtreeRoot {
      anchor: AnchorId,
  }

  pub(crate) struct DetachedChild {
      expected_key: GroupKey,
      subtree: DetachedSubtree,
  }
  ```

  Commit message:

  ```text
  slot: introduce typed child cursors
  ```

* [ ] Convert group insertion to typed cursor API.

  Done means: `insert_new_group` or its replacement accepts `ChildCursor`; it asserts the cursor is a direct-child boundary of the cursor parent; raw index insertion is private implementation detail only.

  Commit message:

  ```text
  slot: type new group insertion cursor
  ```

* [ ] Convert subtree movement to typed cursor API.

  Done means: `move_subtree` or its replacement accepts `ActiveSubtreeRoot` and `ChildCursor`; it asserts the moved root is a later direct sibling of the same parent; it cannot move a parent into its child, a child across parents, or a retained subtree through active movement.

  Commit message:

  ```text
  slot: type active subtree movement
  ```

* [ ] Convert detached restore to typed cursor API.

  Done means: `restore_subtree` or its replacement accepts `ChildCursor` and `DetachedChild`; it asserts root key match, cursor parent ownership, detached anchor state, retained payload state, and direct-child insertion boundary before mutation.

  Commit message:

  ```text
  slot: type detached subtree restore
  ```

* [ ] Hide raw structural helpers.

  Done means: raw `usize` mutation helpers are private to the smallest module scope; writer code uses semantic cursor operations; tests use semantic helpers unless explicitly testing corruption/validation.

  Commit message:

  ```text
  slot: hide raw structural mutation helpers
  ```

* [ ] Add invalid cursor tests.

  Done means: tests cover cross-parent move rejection, restore key mismatch, restore at non-child boundary, moving grandchildren as siblings, restoring active anchors, and duplicate direct sibling keys.

  Commit message:

  ```text
  slot: reject invalid child cursor mutations
  ```

---

## Phase 6 — Make node replacement lifecycle explicit

* [ ] Replace `NodeRecordResult` with `NodeSlotUpdate`.

  Done means: the node API returns `Reused`, `Inserted`, or `Replaced`; a boolean `reused` result no longer exists.

  Required shape:

  ```rust
  pub(crate) enum NodeSlotUpdate {
      Reused {
          id: NodeId,
          generation: u32,
      },
      Inserted {
          id: NodeId,
          generation: u32,
      },
      Replaced {
          old_id: NodeId,
          old_generation: u32,
          new_id: NodeId,
          new_generation: u32,
      },
  }
  ```

  Commit message:

  ```text
  slot: make node slot updates explicit
  ```

* [ ] Wire `NodeSlotUpdate` into emitter lifecycle handling.

  Done means: the emitter/applier path matches all three variants; replaced nodes are removed/detached through the same lifecycle path as other displaced nodes; wrong-type or wrong-generation reuse cannot silently look like normal reuse.

  Commit message:

  ```text
  slot: handle explicit node replacement
  ```

* [ ] Add node replacement regression tests.

  Done means: tests cover reused same node, inserted new node, replaced node id, replaced generation, retained detached node restore, and disposal of replaced nodes. Validation proves no replaced node remains active under the old identity.

  Commit message:

  ```text
  slot: test explicit node replacement
  ```

---

## Phase 7 — Turn scope lookup into a real `ScopeIndex`

* [ ] Replace `scope_anchor_to_group` with `ScopeIndex`.

  Done means: `SlotTable` owns `ScopeIndex`, not a raw `HashMap<ScopeId, AnchorId>` field. The name reflects the actual mapping.

  Required shape:

  ```rust
  pub(crate) struct ScopeIndex {
      by_scope: HashMap<ScopeId, AnchorId>,
  }
  ```

  Commit message:

  ```text
  slot: introduce scope index type
  ```

* [ ] Move scope operations into `ScopeIndex`.

  Done means: assign, remove, active lookup, restore entries, rebuild, shrink, heap/capacity stats, and validation are methods on `ScopeIndex`. Slot table code no longer manipulates the map directly.

  Commit message:

  ```text
  slot: centralize scope index operations
  ```

* [ ] Wire scope index to detach, restore, disposal, and compaction.

  Done means: active scopes are indexed; detached scopes are removed from active lookup but carried by `DetachedSubtree`; restored scopes re-enter the index; disposed scopes are unregistered/invalidated through the existing composer lifecycle.

  Commit message:

  ```text
  slot: wire scope index lifecycle
  ```

* [ ] Add scope identity regression tests.

  Done means: tests cover active lookup, detach, retained inactive invalidation, restore, forced recomposition after restore, disposal, and storage compaction without scope anchor renaming.

  Commit message:

  ```text
  slot: test scope index lifecycle
  ```

---

## Phase 8 — Clean module boundaries and names

* [ ] Move handle and operation types out of misleading `slot_storage.rs`.

  Done means: `GroupKey`, `GroupKeySeed`, `ActiveGroupId`, `ValueSlotId`, `BeginGroupInput`, `GroupStart`, `GroupStartKind`, and `NodeSlotUpdate` live in a correctly named module such as `slot/types.rs` or `slot/ops.rs`. `slot_storage.rs` is deleted if it no longer represents a real storage abstraction.

  Commit message:

  ```text
  slot: move storage operation types into slot modules
  ```

* [ ] Remove stale imports and wrapper surfaces from the old names.

  Done means: there is no compatibility shim just to preserve internal old names; import paths are direct and idiomatic; grep for `slot_storage::`, `GroupId`, `NodeRecordResult`, `compact_*_namespace`, and `payload_locations` returns no stale semantic usage.

  Commit message:

  ```text
  slot: remove stale storage naming
  ```

* [ ] Rename payload-location modules to payload-anchor modules.

  Done means: the old location-only concept is gone; module/file names reflect anchor lifecycle and resolution, not merely active table location lookup.

  Commit message:

  ```text
  slot: rename payload location registry
  ```

* [ ] Update debug snapshots and stats names.

  Done means: debug output reports stable identities and storage capacities separately. It does not imply that id namespace size is the same as active table length. It distinguishes active, detached, invalidated, free, and storage capacity counts.

  Commit message:

  ```text
  slot: clarify identity debug stats
  ```

---

## Phase 9 — Expand deterministic model and stress coverage

* [ ] Extend model tests for group anchor identity.

  Done means: generated operations randomly insert, move, detach, retain, restore, compact storage, and dispose groups while asserting anchor identity stability and generation reuse rules after every step.

  Commit message:

  ```text
  slot: model stable group identities
  ```

* [ ] Extend model tests for payload anchor identity.

  Done means: generated operations randomly create remembered values, replace payload types, detach/retain/restore payloads, compact storage, dispose subtrees, and assert `ValueSlotId` behavior after every step.

  Commit message:

  ```text
  slot: model stable payload identities
  ```

* [ ] Extend model tests for node lifecycle updates.

  Done means: generated operations randomly reuse, insert, replace, detach, restore, and dispose nodes; model state agrees with emitted node lifecycle and `NodeSlotUpdate`.

  Commit message:

  ```text
  slot: model node slot lifecycle
  ```

* [ ] Extend model tests for scope index lifecycle.

  Done means: generated operations randomly assign scopes, detach scoped groups, invalidate retained inactive scopes, restore them, dispose them, and compact storage; active lookup matches model expectations.

  Commit message:

  ```text
  slot: model scope index lifecycle
  ```

* [ ] Run a high-frame deterministic stress profile and commit any required threshold/test updates.

  Done means: `CRANPOSE_SLOT_MODEL_STRESS_FRAMES` is raised for local stress validation; failures are fixed at the root; no perf threshold is loosened without evidence in the commit message.

  Commit message:

  ```text
  slot: strengthen model stress coverage
  ```

---

## Phase 10 — Batch index and location maintenance after identities are correct

* [ ] Add mutation instrumentation for active index refreshes.

  Done means: stats report how many group anchor active-index refreshes, payload active-location refreshes, scope index rebuilds, and segment range updates happen per pass. This item does not optimize behavior yet.

  Commit message:

  ```text
  slot: instrument index refresh work
  ```

* [ ] Batch group anchor active-index refreshes inside writer transactions.

  Done means: repeated structural mutations record dirty active group ranges and refresh anchor active indices once at the end of the safe mutation boundary where possible. Validation still passes after each operation in debug/test paths where invariants are checked.

  Commit message:

  ```text
  slot: batch group anchor index refresh
  ```

* [ ] Batch payload active-location refreshes inside writer transactions.

  Done means: repeated payload insert/remove/move/restore operations record dirty owner/range information and refresh payload active locations once per affected group/range where possible. Payload identity remains stable.

  Commit message:

  ```text
  slot: batch payload anchor location refresh
  ```

* [ ] Batch scope index rebuilds and eliminate unnecessary full rebuilds.

  Done means: scope index updates are incremental for insert, detach, restore, dispose, and move. Full rebuild remains available for validation/debug recovery only, not normal mutation flow.

  Commit message:

  ```text
  slot: batch scope index maintenance
  ```

* [ ] Add performance regression checks for reorder, detach, restore, and compaction.

  Done means: perf tests cover large keyed sibling reorder, deep insert/remove, retained restore, mass conditional removal, and storage compaction. The tests assert stability against accidental full-table rebuilds in hot paths.

  Commit message:

  ```text
  slot: add identity maintenance perf checks
  ```

---

## Phase 11 — Update docs to match the final architecture

* [ ] Update `docs/cranpose_slot_table_v2_design.md`.

  Done means: the design doc explicitly states that compaction never renames active or retained identities; `ActiveGroupId` is transient; `AnchorId` and `PayloadAnchor` are stable semantic identities; `ScopeIndex` owns active scope lookup; node slot updates are explicit.

  Commit message:

  ```text
  docs: document stable slot identities
  ```

* [ ] Remove stale duplicate slot-table documentation.

  Done means: docs no longer describe gap semantics, namespace renumbering, ambiguous `GroupId` stability, or payload anchors as raw locations except as historical rationale clearly marked inactive.

  Commit message:

  ```text
  docs: remove stale slot table guidance
  ```

* [ ] Update repo roadmap status.

  Done means: completed identity refactor items are marked `[x]`; remaining unrelated roadmap items stay separate; the roadmap does not claim full completion until validation is green on the final code.

  Commit message:

  ```text
  docs: update slot identity roadmap status
  ```

---

## Phase 12 — Final audit and convergence commit

* [ ] Run full textual audit for forbidden stale concepts.

  Done means these searches have no stale semantic matches:

  ```bash
  rg "GroupId|NodeRecordResult|compact_.*namespace|payload_locations|next_payload_anchor|next_payload_generation|restored_from_gap|Slot::Gap|legacy|old way|migration"
  ```

  Valid historical mentions must be removed or rewritten unless they are clearly part of inactive historical rationale in docs.

  Commit message:

  ```text
  slot: remove stale identity terminology
  ```

* [ ] Run final full validation and stress gate from a clean worktree.

  Done means:

  ```bash
  git status --short
  ./verify_slot_table.sh
  ./stress_slot_table.sh
  git status --short
  ```

  The worktree is clean except for intentional roadmap/log/doc updates. All logs are read. Every warning or failure is fixed.

  Commit message:

  ```text
  slot: complete stable identity refactor
  ```

* [ ] Final self-review commit.

  Done means: the agent reviews the complete refactor diff against the invariant “records may move; identities must not”; confirms no shortcut compatibility wrappers remain; confirms every checkbox above is `[x]`; confirms all tests and stress gates passed after the final checkbox update.

  Commit message:

  ```text
  slot: finalize identity refactor roadmap
  ```

---

## Completion condition

The roadmap is complete only when every box is `[x]`, the final commit is present, and the latest local run of both commands is green:

```bash
./verify_slot_table.sh
./stress_slot_table.sh
```

The final architecture must satisfy this invariant everywhere:

```text
Storage compaction may move records and shrink backing storage.
Storage compaction must never rename active or retained identities.
ActiveGroupId is transient.
AnchorId is stable group identity.
PayloadAnchor is stable value-slot identity.
Node replacement is explicit.
Scope lookup is owned by ScopeIndex.
```

[1]: https://github.com/samoylenkodmitry/Cranpose "GitHub - samoylenkodmitry/Cranpose: Cranpose is a Jetpack Compose-inspired declarative Rust UI framework. https://crates.io/crates/cranpose · GitHub"
[2]: https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/main/crates/cranpose-core/src/slot_storage.rs "raw.githubusercontent.com"
[3]: https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/main/crates/cranpose-core/src/slot/anchors.rs "raw.githubusercontent.com"
[4]: https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/main/verify_slot_table.sh "raw.githubusercontent.com"
[5]: https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/main/AGENTS.md "raw.githubusercontent.com"
