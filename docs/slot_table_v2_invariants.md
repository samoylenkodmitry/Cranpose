# Slot Table V2 Invariants

Source of truth: `docs/cranpose_slot_table_v2_design.md`

This document is the short operational checklist for Slot Table V2. Keep it small: the design doc explains the architecture, while this file names the invariants that validation, tests, and reviews must protect.

## Active Table

- Active groups form a preorder forest.
- A group's `parent_anchor` is either invalid for a root or points to its active parent.
- A group's `depth` matches its parent stack depth.
- `subtree_len` is the exact active group count covered by that group, including the group itself.
- `subtree_node_count` is the exact active node count covered by that group and its descendants.
- Direct siblings are contiguous inside their parent-bounded range.
- Group identity matching searches only direct siblings, never grandchildren.
- Duplicate explicit sibling keys are invalid.

## Payloads

- Each group owns one contiguous payload range.
- Payload ranges are in table order and cover the payload table without gaps.
- Every payload owner anchor resolves to the active group that owns the payload.
- Every payload location registry entry points back to the exact owner group and payload index.
- Payload kind is explicit at the call site; lifecycle/debug code must not infer semantics from type names.

## Nodes

- Each group owns one contiguous node range.
- Node ranges are in table order and cover the node table without gaps.
- Every node owner anchor resolves to the active group that owns the node record.
- Active node records have `NodeLifecycle::Active`.
- Retained node records have `NodeLifecycle::RetainedDetached`.
- Disposed nodes are not retained by slot storage.
- Skipped-group root node metadata is exact and comes from stored node records, not applier tree scans.

## Anchors

- Every active group anchor resolves to exactly one active group index.
- Anchor registry active count equals active group count.
- Detached anchors belong only to detached or retained subtrees.
- Invalidated anchors do not resolve to active or retained records.
- Free anchors are reusable capacity only; they are not semantic group state.
- Active and retained storage never share the same anchor as active at the same time.

## Retention And Detached Subtrees

- Removed inactive groups are represented as explicit `DetachedSubtree` values, never as gaps or spare active-table capacity.
- Retained subtrees are owned by the composer-side retention system and are absent from active `SlotTable.groups`.
- A retained subtree root has no active parent.
- A retained subtree root key matches the retention key used to store it.
- Retained subtree payload owners refer to anchors inside the detached subtree.
- Retained subtree node owners refer to anchors inside the detached subtree.
- Retained subtree scopes are inactive and are not present in the active scope index.
- Restoring a subtree reactivates anchors and scopes before the group participates in recomposition.
- Disposing a detached subtree invalidates anchors, unregisters scopes, drops payloads, and removes nodes.

## Scope Lookup

- Active scope lookup is indexed by `ScopeId`.
- Slot storage never scans all groups to find a scope.
- The active scope index maps every active group `scope_id` to the exact active group anchor.
- Detached scopes are routed by runtime retention state, not by active slot-table lookup.
- Invalidating an inactive retained scope keeps it dirty and recomposes it when it is restored.

## Validation Expectations

- `SlotTable::validate()` covers active preorder, parent/depth structure, subtree spans, payload/node ranges, ownership, active anchors, active scope index, and duplicate sibling keys.
- Retained-state validation must cover detached anchors, retained scopes, retained node lifecycle, retained root parentage, and retained-key/root-key agreement.
- Debug and test builds should validate after composition operations that mutate slot structure.
- A validation failure should identify the violated invariant locally instead of allowing a later recomposition panic.
