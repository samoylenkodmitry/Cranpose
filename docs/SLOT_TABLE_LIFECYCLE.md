# Slot Table Lifecycle Contract

This document defines the current lifecycle invariants for slot groups, emitted nodes,
payloads, and recomposition scopes. The code paths named here are the authority for
each transition; validation code must reject states outside this contract.

## Active Groups

An active group is present in `SlotTable.groups`, has an active group anchor in
`AnchorRegistry`, and may have an active scope entry in `ScopeIndex`.

Group transitions:

- `Active -> Detached`: `SlotTable::detach_subtree_at_index_internal` removes the
  group segment from the active table, clears active group and scope indexes, marks
  payload anchors detached, and normalizes the detached root parent to
  `AnchorId::INVALID`.
- `Detached -> Restored`: `SlotTable::restore_subtree` verifies that every group
  and payload anchor is detached, inserts the segment back into `SlotTable.groups`,
  refreshes group indexes, restores scope index entries, updates ancestor spans,
  and refreshes payload anchor locations.
- `Detached -> Disposed`: `Composer::dispose_detached_subtree_in_host`,
  `ComposerRuntimeState::dispose_retained_subtrees_for_host`, and root pass cleanup
  dispose detached nodes, remove or deactivate scopes, invalidate detached anchors,
  and queue payload disposal.
- `Disposed -> Invalidated`: `SlotTable::invalidate_detached_subtree_anchors`
  removes detached group and payload anchors from their registries and makes stale
  handles unresolvable until their IDs are reused with bumped generations.

Active group anchors must never resolve to detached storage. Detached group anchors
must never remain in the active group index or active scope index.

## Nodes

Node records are owned by groups and move with detached subtrees.

Node lifecycle states:

- `Active`: the node is part of an active slot subtree or a freshly detached subtree
  that has not been retained. Active detached nodes are disposed if the subtree is
  not retained.
- `RetainedDetached`: `RetentionManager::insert` marks retained subtree nodes with
  this lifecycle after the composer detaches root nodes from their active parent in
  the applier. Retained nodes remain allocated and are not removed from the applier.
- `Disposed/removed from applier`: disposal paths call
  `dispose_detached_subtree_now` or `Composer::dispose_detached_nodes` for root
  nodes before invalidating anchors and queuing payload drops.

`RetentionManager::take` and `SlotTable::restore_subtree` mark retained nodes active
again before the subtree is used as active storage.

## Payloads

Payload records are stored in group-owned payload segments and identified by
`PayloadAnchor`.

Payload lifecycle:

- Active payload: stored in `SlotTable.payloads` and registered as an active payload
  anchor location `(owner, index)`.
- Detached retained payload: moved into `DetachedSubtree.payloads` with its anchor
  marked detached. It remains owned by the detached subtree and must not resolve
  through `SlotTable::read_value`.
- Deferred drop: replacing, removing, or disposing payload records converts the old
  value into `DeferredDrop` and queues it in `SlotLifecycleCoordinator`.
- Final disposal: `SlotLifecycleCoordinator::flush_pending_drops` drains queued
  drops. `dispose_slot_table` flushes pending drops, drains all active payloads,
  queues them, and flushes again.

Restoring a detached subtree must refresh payload anchor locations after segment
insertion. Payload anchors must be invalidated before their IDs can be reused.

## Scopes

Scope IDs tie active groups to recomposition scopes in the runtime registry.

Scope lifecycle:

- Active scope: the group is active, `ScopeIndex` maps the scope ID to the group
  anchor, and `ComposerRuntimeState` owns the `RecomposeScope`.
- Inactive retained scope: the subtree is retained, the scope remains in the runtime
  registry, but it is deactivated and removed from the active `ScopeIndex`.
- Restored invalid scope: restoring retained content reuses the scope and restores
  its active group anchor; the scope is marked invalid so the restored content can
  recompose under the active table again.
- Removed/disposed scope: disposal removes the scope from
  `ComposerRuntimeState::scope_registry`, deactivates it, and clears its group
  anchor.

The slot table only indexes active scopes. Routing detached retained scopes remains
the responsibility of the composer runtime state.

## Validation Boundaries

`SlotTable::debug_verify`, detached subtree validation, and retention validation
must agree with this document:

- active indexes contain only active groups and active payload locations;
- retained subtrees contain only detached group and payload anchors;
- retained nodes use `RetainedDetached`;
- retained scopes are absent from the active scope index;
- stale anchors and value slots fail rather than aliasing unrelated records.
