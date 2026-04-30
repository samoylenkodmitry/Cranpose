Below is a checklist roadmap an agent can follow. Rule for every `[ ]`: mark `[x]` **only after** implementation, self-review, validation, and commit are complete.

# Slot Table Hardening Roadmap

## Agent operating protocol

* [ ] Create a working branch from `main`.

    * [ ] Implement only one roadmap item or tightly related group per commit.
    * [ ] Self-review the diff before validating.
    * [ ] Run formatting and tests.
    * [ ] Commit with a focused message.
    * [ ] Mark the checklist item `[x]` only after the commit lands.

* [ ] Use this validation baseline for every implementation commit:

    * [ ] `cargo fmt --all --check`
    * [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    * [ ] `cargo test --workspace --all-features`
    * [ ] Targeted slot-table tests when relevant:

        * [ ] `cargo test -p cranpose-core slot::`
        * [ ] `cargo test -p cranpose-core retention`
        * [ ] `cargo test -p cranpose-core recompose`
        * [ ] `cargo test -p cranpose-core subcompose`

---

# Phase 1 — Make slot identity misuse fail early

## 1. Add debug storage identity to value slots

* [x] Implement debug-only storage identity on `ValueSlotId`.

    * [x] Add `#[cfg(any(test, debug_assertions))] storage_id: usize` to `ValueSlotId`.
    * [x] Update `ValueSlotId::new(...)` or create `ValueSlotId::new_for_table(...)`.
    * [x] Thread `SlotTable::storage_id()` into value-slot creation.
    * [x] In `SlotTable::checked_value_slot`, assert that the slot belongs to the current table in debug/test builds.
    * [x] Keep release layout minimal if desired.

* [x] Add tests.

    * [x] Same-table value slot read/write still succeeds.
    * [x] Cross-table value-slot access panics or reports a clear invariant failure in debug/test.
    * [x] Detached/restored retained value slot still resolves through the correct table.
    * [x] Type mismatch behavior remains unchanged.

* [x] Self-review.

    * [x] Confirm no public API break unless intentional.
    * [x] Confirm `ValueSlotHandle<'pass>` still compiles cleanly.
    * [x] Confirm no accidental clone/copy layout issue.

* [x] Validate and commit.

    * [x] Run full validation baseline.
    * [x] Commit: `slot: guard value slots with debug storage identity`
    * [x] Mark this item `[x]`.

---

# Phase 2 — Document lifecycle invariants

## 2. Add authoritative slot lifecycle contract

* [x] Add a lifecycle contract document or module comment.

    * [x] Location option: `docs/SLOT_TABLE_LIFECYCLE.md`.
    * [x] Location option: module-level comments in `crates/cranpose-core/src/slot/lifecycle.rs`.
    * [x] Include active group lifecycle:

        * [x] `Active -> Detached`
        * [x] `Detached -> Restored`
        * [x] `Detached -> Disposed`
        * [x] `Disposed -> Invalidated`
    * [x] Include node lifecycle:

        * [x] `Active`
        * [x] `RetainedDetached`
        * [x] `Disposed/removed from applier`
    * [x] Include payload lifecycle:

        * [x] Active payload.
        * [x] Detached retained payload.
        * [x] Deferred drop.
        * [x] Final disposal.
    * [x] Include scope lifecycle:

        * [x] Active scope.
        * [x] Inactive retained scope.
        * [x] Restored invalid scope.
        * [x] Removed/disposed scope.

* [x] Cross-link code comments.

    * [x] `slot/detach.rs`
    * [x] `slot/lifecycle.rs`
    * [x] `retention.rs`
    * [x] `composer.rs`

* [x] Self-review.

    * [x] Confirm the doc describes current behavior, not aspirational behavior.
    * [x] Confirm every lifecycle transition has a corresponding code path.
    * [x] Confirm terminology matches code names.

* [x] Validate and commit.

    * [x] Run formatting/tests if comments/docs only still touch Rust comments.
    * [x] Commit: `docs: define slot table lifecycle invariants`
    * [x] Mark this item `[x]`.

---

# Phase 3 — Harden payload-anchor refresh invariants

## 3. Make deferred payload-location refresh explicit

* [x] Add explicit pending-refresh diagnostics.

    * [x] Add a debug/test-only flag or helper on `SlotWriteSessionState`.
    * [x] Track whether `payload_location_refreshes` is non-empty.
    * [x] Add a small helper like `has_pending_payload_location_refreshes()`.

* [x] Assert safe access boundaries.

    * [x] Before direct value read/write from writer-sensitive paths, assert refreshes have been flushed where required.
    * [x] Keep intentional pre-flush operations documented.
    * [x] Ensure `begin_group`, `begin_recompose_at_scope`, `finish_group_body`, `finalize_pass`, and writer validation remain flush points.

* [x] Add tests.

    * [x] Insert multiple payloads into the same group and verify coalesced refresh start is minimal.
    * [x] Verify value reads after flush resolve correct payload anchors.
    * [x] Verify finishing a group flushes pending refreshes.
    * [x] Verify validation flushes or catches pending invalid state deterministically.

* [x] Self-review.

    * [x] Confirm no extra refresh work on hot path beyond intended debug checks.
    * [x] Confirm mutation debug stats still make sense.
    * [x] Confirm no borrow checker workaround creates hidden mutable aliasing risk.

* [x] Validate and commit.

    * [x] Run full validation baseline.
    * [x] Commit: `slot: make deferred payload anchor refresh explicit`
    * [x] Mark this item `[x]`.

---

# Phase 4 — Centralize structural mutation recipes

## 4. Add mutation operation checklists

* [x] Add internal comments or helper structs documenting mutation order.

    * [x] `detach_subtree`
    * [x] `restore_subtree`
    * [x] `move_subtree`
    * [x] payload insertion/removal
    * [x] node insertion/removal

* [x] Introduce a lightweight mutation guard in debug/test builds.

    * [x] Optional: `SlotMutationGuard`.
    * [x] On drop, run `debug_assert_valid_after(operation)` when diagnostics are enabled.
    * [x] Avoid using guard where validation would recursively borrow or cause large overhead.

* [x] Reduce duplicated mutation sequencing.

    * [x] Identify repeated patterns:

        * [x] segment extraction/restoration
        * [x] active-index refresh
        * [x] payload-location refresh
        * [x] scope-index update
        * [x] ancestor span update
    * [x] Extract helper only where it improves clarity.
    * [x] Do not over-abstract the current readable flow.

* [x] Add tests.

    * [x] Move later sibling to earlier cursor.
    * [x] Detach middle subtree with payloads and nodes.
    * [x] Restore retained subtree with scopes, payloads, and nodes.
    * [x] Remove tail payloads/nodes during recomposition.
    * [x] Root-level detach during pass finalization.

* [x] Self-review.

    * [x] Confirm helpers preserve exact operation ordering.
    * [x] Confirm validation still catches intentionally corrupted fixtures.
    * [x] Confirm no production-only behavior depends on debug guard.

* [x] Validate and commit.

    * [x] Run full validation baseline.
    * [x] Commit: `slot: document and guard structural mutation recipes`
    * [x] Mark this item `[x]`.

---

# Phase 5 — Strengthen anchor-registry test matrix

## 5. Add shared anchor registry behavior coverage

* [x] Add tests for `AnchorRegistry`.

    * [x] Allocate active anchor.
    * [x] Mark detached.
    * [x] Invalidate detached.
    * [x] Reuse ID with bumped generation.
    * [x] Reject stale generation.
    * [x] Preserve dense hot-path capacity.
    * [x] Handle sparse IDs without dense explosion.
    * [x] Validate active count, detached count, invalidated/free count.

* [x] Add tests for `PayloadAnchorRegistry`.

    * [x] Allocate payload anchor.
    * [x] Set active location.
    * [x] Mark detached.
    * [x] Invalidate and reuse with bumped generation.
    * [x] Coalesce invalidated payload anchor ranges.
    * [x] Reject stale generation.
    * [x] Preserve dense hot-path capacity.
    * [x] Handle sparse IDs.
    * [x] Validate active/detached/free counts.

* [x] Add retained-subtree compaction tests.

    * [x] Group anchor compaction with retained subtrees.
    * [x] Payload anchor compaction with retained subtrees.
    * [x] Ensure retained anchors are not invalidated or reused prematurely.

* [x] Self-review.

    * [x] Confirm tests cover both registries without forcing identical implementations.
    * [x] Confirm generation semantics are explicit.
    * [x] Confirm stale handles cannot resolve after invalidation.

* [x] Validate and commit.

    * [x] Run full validation baseline.
    * [x] Commit: `slot: expand anchor registry invariant coverage`
    * [x] Mark this item `[x]`.

---

# Phase 6 — Clarify `move_subtree` semantics

## 6. Rename or document later-sibling-only move

* [x] Decide whether to rename.

    * [x] Option A: rename `move_subtree` to `move_later_sibling_subtree_to_cursor`.
    * [x] Option B evaluated; the renamed internal primitive carries the clear contract doc comment.
    * [x] Prefer rename if call sites are few and internal-only.

* [x] Update assertions and messages.

    * [x] State that the root must be a direct child of the cursor parent.
    * [x] State that only moving a later direct sibling earlier is supported.
    * [x] State that this is writer-driven keyed sibling reordering, not a general tree move.

* [x] Add tests.

    * [x] Moving later sibling before earlier sibling succeeds.
    * [x] Moving same cursor is no-op.
    * [x] Moving across parents fails.
    * [x] Moving an earlier sibling later fails or is unsupported by explicit assertion.
    * [x] Moving a grandchild as if it were a sibling fails.

* [x] Self-review.

    * [x] Confirm public/internal API impact is acceptable.
    * [x] Confirm error messages match actual constraints.
    * [x] Confirm keyed reorder behavior remains unchanged.

* [x] Validate and commit.

    * [x] Run full validation baseline.
    * [x] Commit: `slot: clarify keyed sibling move constraints`
    * [x] Mark this item `[x]`.

---

# Phase 7 — Retention and applier integration hardening

## 7. Add retained-node lifecycle integration tests

* [ ] Add tests for retaining inactive groups.

    * [ ] Compose group with remembered payload and emitted node.
    * [ ] Remove group with `RetainWhenInactive`.
    * [ ] Verify subtree is retained.
    * [ ] Verify node lifecycle is `RetainedDetached`.
    * [ ] Verify payload is not dropped.
    * [ ] Verify scope is inactive but still registered.

* [ ] Add restore tests.

    * [ ] Restore retained group by same parent scope and group key.
    * [ ] Verify node lifecycle returns to active.
    * [ ] Verify remembered payload is reused.
    * [ ] Verify restored invalid scope forces recomposition.
    * [ ] Verify active scope index is restored.

* [ ] Add eviction tests.

    * [ ] Set max retained subtree count.
    * [ ] Retain more groups than budget.
    * [ ] Verify eviction disposes nodes.
    * [ ] Verify payload drops are queued/flushed.
    * [ ] Verify anchors are invalidated.
    * [ ] Verify scopes are removed or deactivated correctly.

* [ ] Add host reset tests.

    * [ ] Retain subtree in subcompose/secondary host.
    * [ ] Reset host.
    * [ ] Verify retained subtrees are disposed before host ownership is cleared.
    * [ ] Verify scope registry has no stale host references.

* [ ] Self-review.

    * [ ] Confirm tests exercise composer, retention, slot table, and applier together.
    * [ ] Confirm no test only validates internal counters while missing user-visible behavior.
    * [ ] Confirm retained nodes are not accidentally removed from applier.

* [ ] Validate and commit.

    * [ ] Run full validation baseline.
    * [ ] Commit: `retention: test retained subtree node and scope lifecycle`
    * [ ] Mark this item `[x]`.

---

# Phase 8 — Scope index robustness

## 8. Strengthen scope index behavior

* [ ] Add tests for scope assignment.

    * [ ] New group gets scope.
    * [ ] Existing group keeps scope across reuse.
    * [ ] Moved group keeps scope.
    * [ ] Detached group scope is removed from active index.
    * [ ] Restored group scope is restored to active index.
    * [ ] Disposed group scope is removed from runtime registry.

* [ ] Add duplicate scope tests.

    * [ ] Assigning the same scope to a different active group should fail.
    * [ ] Restoring a subtree whose scope conflicts with an active group should fail.

* [ ] Add recomposition entry tests.

    * [ ] `begin_recompose_at_scope` finds active scoped group.
    * [ ] `begin_recompose_at_scope` returns `None` for detached retained scope.
    * [ ] Restored invalid scope can later recompose normally.

* [ ] Self-review.

    * [ ] Confirm slot table only indexes active scopes.
    * [ ] Confirm detached scope routing remains composer/runtime-state responsibility.
    * [ ] Confirm no active-scope scan was reintroduced.

* [ ] Validate and commit.

    * [ ] Run full validation baseline.
    * [ ] Commit: `slot: harden active scope index invariants`
    * [ ] Mark this item `[x]`.

---

# Phase 9 — Performance regression coverage

## 9. Add slot table performance regression tests

* [ ] Add large keyed sibling reorder scenario.

    * [ ] Compose many keyed siblings.
    * [ ] Reverse or rotate order.
    * [ ] Verify sibling index path works.
    * [ ] Verify mutation stats do not show pathological unexpected refreshes.

* [ ] Add large detach/restore scenario.

    * [ ] Large subtree with groups, payloads, nodes, and scopes.
    * [ ] Detach and retain.
    * [ ] Restore.
    * [ ] Validate exact group spans and payload/node ranges.

* [ ] Add repeated tail removal scenario.

    * [ ] Compose many payloads/nodes.
    * [ ] Recompose with shorter tails repeatedly.
    * [ ] Verify compaction hints trigger at thresholds.
    * [ ] Verify storage compaction does not invalidate retained identities.

* [ ] Add optional ignored benchmark-style tests.

    * [ ] Mark as `#[ignore]` if too slow for normal CI.
    * [ ] Keep deterministic and not timing-sensitive unless behind a benchmark feature.

* [ ] Self-review.

    * [ ] Confirm tests detect structural regressions, not machine-specific timing.
    * [ ] Confirm mutation debug stats are asserted only where stable.
    * [ ] Confirm large cases remain reasonable in CI.

* [ ] Validate and commit.

    * [ ] Run full validation baseline.
    * [ ] Commit: `slot: add structural performance regression coverage`
    * [ ] Mark this item `[x]`.

---

# Phase 10 — Final validation and cleanup

## 10. Run full project validation

* [ ] Run full workspace validation.

    * [ ] `cargo fmt --all --check`
    * [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    * [ ] `cargo test --workspace --all-features`
    * [ ] Any project-specific CI commands from README or workflows.

* [ ] Run diagnostics-enabled tests.

    * [ ] Enable slot validation diagnostics if supported.
    * [ ] Run targeted slot, retention, and recompose tests.
    * [ ] Confirm debug assertions pass.

* [ ] Review final diff.

    * [ ] Check public API changes.
    * [ ] Check docs match implementation.
    * [ ] Check commit history is focused.
    * [ ] Check no temporary debug prints or ignored failing tests remain.
    * [ ] Check no broad unrelated refactors slipped in.

* [ ] Commit final cleanup if needed.

    * [ ] Commit: `slot: finalize slot table hardening roadmap`
    * [ ] Mark this item `[x]`.

---

# Completion criteria

* [ ] Every roadmap item is marked `[x]`.
* [ ] Every `[x]` item has a corresponding commit.
* [ ] Full workspace validation passes.
* [ ] Slot table lifecycle docs exist and match implementation.
* [ ] Cross-table value-slot misuse is guarded.
* [ ] Payload-anchor refresh invariants are explicit.
* [ ] Structural mutation operations are documented or guarded.
* [ ] Anchor registries have stale-handle and reuse coverage.
* [ ] Retention, restore, eviction, and host reset are covered by integration tests.
* [ ] No active-scope scanning or semantic gap behavior has been reintroduced.
