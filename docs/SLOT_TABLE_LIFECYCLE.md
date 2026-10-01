# Slot Table Lifecycle Contract

This document defines the current lifecycle invariants for slot groups, emitted nodes,
payloads, and recomposition scopes. The code paths named here are the authority for
each transition; validation code must reject states outside this contract.

## Active Groups

An active group is present in `SlotTable.groups`, has an active group anchor in
`AnchorRegistry`, and may hold its `RecomposeScope`.

Group transitions:

- `Active -> Detached`: `SlotTable::detach_subtree_at_index_internal` removes the
  group segment from the active table, clears the active group index, marks
  payload anchors detached, and normalizes the detached root parent to
  `AnchorId::INVALID`. The detached groups keep their scopes.
- `Detached -> Restored`: `SlotTable::restore_subtree` verifies that every group
  and payload anchor is detached, inserts the segment back into `SlotTable.groups`,
  refreshes group indexes, updates ancestor spans, and refreshes payload anchor
  locations.
- `Detached -> Disposed`: `Composer::dispose_detached_subtree_in_host`,
  `ComposerRuntimeState::dispose_retained_subtrees_for_host`, and root pass cleanup
  dispose detached nodes, deactivate the scopes the groups hold, invalidate detached
  anchors, and queue the groups and payloads for disposal.
- `Disposed -> Invalidated`: `SlotTable::invalidate_detached_subtree_anchors`
  removes detached group and payload anchors from their registries and makes stale
  handles unresolvable until their IDs are reused with bumped generations.

Active group anchors must never resolve to detached storage. Detached group anchors
must never remain in the active group index.

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

`RetentionManager::take_after_restore_preflight` runs the active-table restore
checks before a retained subtree is removed from retention. `RetentionManager::take`
then returns the subtree with nodes still marked `RetainedDetached`; only
`SlotTable::restore_subtree` marks those nodes active, after restore preflight has
accepted the target cursor and anchors.

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
Diagnostics distinguish occupied invalidated payload-anchor slots from reusable
free payload-anchor IDs; disposed payload anchors contribute to the free count.

## Scopes

A group record holds its `RecomposeScope`, and the scope records the anchor of its
group. A group start takes the scope from the record; a recomposition reaches the
group through the scope's anchor and accepts it only when that group holds the same
scope.

Scope lifecycle:

- Active scope: the group is active and holds the scope; the scope's anchor is the
  group's anchor.
- Inactive retained scope: the subtree is retained, its groups keep their scopes,
  and the scopes are deactivated. Their anchors are detached, so they reach no
  active group.
- Restored invalid scope: restoring retained content reuses the scopes its groups
  hold; the root scope is forced to recompose so the restored content composes
  under the active table again. A subtree adopted from another table gets new
  anchors, and its scopes follow them.
- Disposed scope: disposal deactivates the scopes the detached groups hold, and the
  scopes are dropped with the group records unless something else holds them.
  Clearing a slot host takes every scope out of its table and deactivates it, so
  whatever composes the table next starts with fresh scopes.

## Validation Boundaries

`SlotTable::debug_verify`, detached subtree validation, and retention validation
must agree with this document:

- active indexes contain only active groups and active payload locations;
- retained subtrees contain only detached group and payload anchors;
- retained nodes use `RetainedDetached`;
- retained scopes reach no active group;
- stale anchors and value slots fail rather than aliasing unrelated records.
