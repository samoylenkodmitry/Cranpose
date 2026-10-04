# Slot table architecture

The slot table keeps active composition records in ordered storage and keeps
payloads, emitted nodes and anchors in separately indexed stores. Removed
content moves into an explicit `DetachedSubtree`; the composer then restores,
retains or disposes each subtree. Generational anchors keep stale handles
separate from later records.

## Current structure

[`SlotTable`](../crates/cranpose-core/src/slot/table.rs) owns the active group
vector, payload store, node vector, group and payload anchor registries, and
movable-content index. A write session updates these stores together. Group
records hold contiguous ranges for their payloads and nodes; subtree spans let
the writer address active children through the group vector.

Direct sibling identity uses each group's static key, optional explicit key
and ordinal. Conditional branches contribute location folds to the keys opened
inside them. A branch contributes keys to its descendants. Movable content uses
an exact key to preserve identity across parent changes.

Detached subtrees carry their group, payload and node records with their
anchors. Retention keeps the subtree outside the active table. Restore checks
the destination and anchors before insertion. Disposal drops payloads, removes
nodes, deactivates scopes and invalidates anchors.

## Current references

- [`slot_table_invariants.md`](slot_table_invariants.md) lists the active and
  retained-state invariants reviewers and validators protect.
- [`SLOT_TABLE_LIFECYCLE.md`](SLOT_TABLE_LIFECYCLE.md) describes group,
  payload, node and scope transitions.
- [`slot/mod.rs`](../crates/cranpose-core/src/slot/mod.rs) maps the slot
  modules; writer and validation code live in the same directory.
- [`movable_tests.rs`](../crates/cranpose-core/src/tests/movable_tests.rs)
  covers observable movable-content behavior.

The former V2 implementation checklist is retired. Current code and the two
contract documents above define the behavior.
