# Slot Table V2 Roadmap

Source: `docs/cranpose_slot_table_v2_design.md`

## Rewrite Principles

- [ ] Replace the current gap-based model instead of patching it.
- [ ] Separate active group structure, remembered payload storage, emitted node identity, and inactive/restorable retention.
- [ ] Keep dormant groups as explicit `DetachedSubtree` values owned by composer-side retention.
- [ ] Ensure a gap, free slot, or spare capacity is never treated as a dormant group.
- [ ] Remove `Slot::Gap` carrying semantic group metadata.
- [ ] Remove `restored_from_gap`.
- [ ] Remove `last_start_was_gap`.
- [ ] Remove `has_gap_children`.
- [ ] Remove stale group length as physical extent semantics.
- [ ] Remove `SEARCH_BUDGET` and `EXTENDED_SEARCH_BUDGET` rescue scans.
- [ ] Remove `step_back` cursor repair.
- [ ] Remove `advance_after_node_read` cursor repair.
- [ ] Remove scope lookup by scanning all slots.

## Correctness Goals

- [ ] Active groups form one preorder forest.
- [ ] Every group has an exact active subtree size.
- [ ] Parent and child boundaries are exact and parent-bounded.
- [ ] Payload ranges are exact and owned by a group.
- [ ] Group identity is matched only among siblings of the current parent.
- [ ] A detached group is not present in the active table.
- [ ] Retained groups are explicit detached objects, not gaps.
- [ ] Anchors resolve to active groups, payloads, and nodes or fail cleanly.
- [ ] Scope lookup is indexed instead of scanning slots.
- [ ] Node lifecycle is explicit: active, retained-detached, or disposed.

## Architectural Goals

- [ ] Keep the slot table responsible for structure and payload storage.
- [ ] Keep the composer responsible for lifecycle decisions.
- [ ] Keep the applier responsible for concrete node objects.
- [ ] Make retention policy-driven and explicit.
- [ ] Ensure skipping and recomposition never depend on stale physical lengths.
- [ ] Make keyed sibling reordering deterministic.
- [ ] Optimize the first V2 pass for clarity and invariants over raw speed.
- [ ] Allow later chunking, packed arrays, or internal gap buffers only if semantics stay unchanged.

## API Goals

- [ ] Replace `restored_from_gap: bool` with semantic `GroupStartKind`.
- [ ] Replace `finalize_current_group() -> bool` with `finish_group_body() -> FinishGroupResult`.
- [ ] Remove `step_back` from the storage trait.
- [ ] Remove `advance_after_node_read` from the storage trait.
- [ ] Add explicit detach operations.
- [ ] Add explicit restore operations.
- [ ] Add storage validation methods for tests and debugging.
- [ ] Keep public `remember`, `useState`, `with_key`, and composable macro behavior as stable as possible while allowing internal API breakage.

## Non-Goals

- [ ] Do not keep `Slot::Gap` semantics.
- [ ] Do not keep old group length behavior.
- [ ] Do not preserve old rescue-scan logic.
- [ ] Do not preserve old experimental backend internals.
- [ ] Do not make the storage trait object-safe just to preserve the old shape.
- [ ] Do not optimize memory packing before correctness.
- [ ] Do not keep undocumented internal debug output unchanged.

## Core Concepts To Preserve

- [ ] Model an active group as a group currently present in the active composition tree.
- [ ] Model `DetachedSubtree` as an owned inactive subtree removed from the active table.
- [ ] Ensure detached subtrees contain group records, payload records, node IDs, scope IDs, and anchor metadata needed for restore or disposal.
- [ ] Ensure detached subtrees have no active parent.
- [ ] Model a retained group as a detached subtree intentionally kept alive by retention policy.
- [ ] Support retained groups for tab pages, reusable lazy or subcompose items, and precomposed content waiting to activate.
- [ ] Model a disposed group as a detached subtree whose payloads, effects, anchors, scopes, and nodes are fully cleaned up.
- [ ] Use `GroupKey { static_key, explicit_key, ordinal }` as the internal group identity model.
- [ ] Keep `static_key` as the usual call-site key.
- [ ] Keep `explicit_key` as the `with_key` or list item key hash.
- [ ] Use `ordinal` to disambiguate duplicate unkeyed sibling calls under the same parent.
- [ ] Make long-term `with_key` identity be source-location key plus user key rather than replacing source location with only the user hash.

## Target Module Layout

- [ ] Replace the current slot table layout with `src/slot_storage.rs` as the V2 trait and public-ish handle definitions.
- [ ] Replace or wrap `src/slot_backend.rs` around the new `SlotTable`.
- [ ] Add `src/slot/mod.rs`.
- [ ] Add `src/slot/types.rs`.
- [ ] Add `src/slot/table.rs`.
- [ ] Add `src/slot/writer.rs`.
- [ ] Add `src/slot/reader.rs`.
- [ ] Add `src/slot/groups.rs`.
- [ ] Add `src/slot/payload.rs`.
- [ ] Add `src/slot/nodes.rs`.
- [ ] Add `src/slot/anchors.rs`.
- [ ] Add `src/slot/scope_index.rs`.
- [ ] Add `src/slot/detach.rs`.
- [ ] Add `src/slot/validate.rs`.
- [ ] Add `src/retention.rs`.
- [ ] Update `lib.rs` re-exports for `SlotTable`, `SlotStorage`, handles, and public APIs from the new modules.
- [ ] Delete or temporarily disable `chunked_slot_storage.rs`.
- [ ] Delete or temporarily disable `hierarchical_slot_storage.rs`.
- [ ] Delete or temporarily disable `split_slot_storage.rs`.
- [ ] If the old experimental backends remain temporarily, turn them into thin wrappers over `SlotTable`.

## Core Data Model

- [ ] Use generational `GroupId`.
- [ ] Use generational `ValueSlotId`.
- [ ] Use generational `GroupAnchor`.
- [ ] Prefer anchor-based `ValueSlotId` resolution when it is exposed outside one composition frame.
- [ ] Implement `SlotTable { groups, payloads, nodes, anchors, scopes, writer, version }`.
- [ ] Allow the first implementation to use plain `Vec` storage and `Vec::splice` for subtree edits.
- [ ] Implement `GroupRecord` with `key`.
- [ ] Implement `GroupRecord` with `parent`.
- [ ] Implement `GroupRecord` with `depth`.
- [ ] Implement `GroupRecord` with exact active `subtree_len`.
- [ ] Implement `GroupRecord` with `payload_start`.
- [ ] Implement `GroupRecord` with `payload_len`.
- [ ] Implement `GroupRecord` with `node_start`.
- [ ] Implement `GroupRecord` with `node_len`.
- [ ] Implement `GroupRecord` with aggregate `subtree_node_count`.
- [ ] Implement `GroupRecord` with `anchor`.
- [ ] Implement `GroupRecord` with `scope_id`.
- [ ] Implement `GroupRecord` with `flags`.
- [ ] Implement `GroupRecord` with `generation`.
- [ ] Ensure `subtree_len` always means exact active group count and never physical extent including holes.
- [ ] Implement `PayloadTable { records }`.
- [ ] Implement `PayloadRecord` with `owner`.
- [ ] Implement `PayloadRecord` with `anchor`.
- [ ] Implement `PayloadRecord` with `type_id`.
- [ ] Implement `PayloadRecord` with `kind`.
- [ ] Implement `PayloadRecord` with `value`.
- [ ] Implement `PayloadKind::Remember`.
- [ ] Implement `PayloadKind::Param`.
- [ ] Implement `PayloadKind::Return`.
- [ ] Implement `PayloadKind::Effect`.
- [ ] Implement `PayloadKind::Scope`.
- [ ] Implement `PayloadKind::Internal`.
- [ ] Keep remembered state in storage while leaving retain-vs-dispose lifecycle decisions to the composer.
- [ ] Implement `NodeTable { records }`.
- [ ] Implement `NodeRecord` with `owner`.
- [ ] Implement `NodeRecord` with `node_id`.
- [ ] Implement `NodeRecord` with `generation`.
- [ ] Store node records in a dedicated node table instead of mixing them with groups and values.
- [ ] Make each group own a range of directly emitted node records.
- [ ] Keep aggregate subtree node count on groups for skip and attach operations.

## SlotStorage V2 Trait

- [ ] Replace the storage trait with semantic group operations.
- [ ] Add `begin_group(&mut self, input: BeginGroupInput) -> GroupStart<Self::Group>`.
- [ ] Add `finish_group_body(&mut self) -> FinishGroupResult`.
- [ ] Add `end_group(&mut self)`.
- [ ] Add `skip_group(&mut self) -> SkippedGroup`.
- [ ] Add `detach_unvisited_children(&mut self) -> Vec<DetachedSubtree>`.
- [ ] Add `restore_detached_at_cursor(&mut self, subtree: DetachedSubtree) -> RestoreResult<Self::Group>`.
- [ ] Add `set_group_scope(&mut self, group: Self::Group, scope: ScopeId)`.
- [ ] Add `begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<RecomposeStart<Self::Group>>`.
- [ ] Add `end_recompose(&mut self)`.
- [ ] Add `value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot`.
- [ ] Add `read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T`.
- [ ] Add `read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T`.
- [ ] Add `write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T)`.
- [ ] Add `remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T>`.
- [ ] Add `record_node(&mut self, id: NodeId) -> NodeRecordResult`.
- [ ] Add `nodes_in_current_group(&self) -> Vec<NodeId>`.
- [ ] Add `reset(&mut self)`.
- [ ] Add `validate(&self) -> Result<(), SlotInvariantError>`.
- [ ] Add `debug_snapshot(&self) -> SlotDebugSnapshot`.
- [ ] Remove `peek_node`.
- [ ] Remove `advance_after_node_read`.
- [ ] Remove `step_back`.
- [ ] Remove `finalize_current_group() -> bool`.
- [ ] Remove `flush` as an anchor-rebuild workaround.
- [ ] If `flush` remains temporarily, make it a no-op or only apply queued structural edits instead of repairing dirty anchors from hacks.
- [ ] Add `BeginGroupInput { key, restored }`.
- [ ] Add `GroupStart { group, anchor, kind, scope_id }`.
- [ ] Add `GroupStartKind::Inserted`.
- [ ] Add `GroupStartKind::Reused`.
- [ ] Add `GroupStartKind::Moved`.
- [ ] Add `GroupStartKind::Restored`.
- [ ] Make `Restored` mean composer-supplied detached subtree insertion rather than gap discovery.
- [ ] Add `FinishGroupResult { detached_children, structure_changed, direct_nodes, subtree_nodes }`.

## Writer Model

- [ ] Route all mutation through a writer state rather than random helper methods that patch cursors.
- [ ] Implement `WriterState { cursor, payload_cursor, node_cursor, stack }`.
- [ ] Implement `Frame { group_index, parent, old_end, next_child, payload_start, payload_cursor, node_start, node_cursor, sibling_index }`.
- [ ] Define each current frame as the state that decides whether the next child is reused, moved, restored, or inserted.
- [ ] In `begin_group`, restore an explicitly supplied detached subtree before any active-table matching.
- [ ] In `begin_group`, reuse the group at the expected direct-child position when parent and key match.
- [ ] In `begin_group`, search later direct siblings for a move candidate when the expected child does not match.
- [ ] In `begin_group`, insert a new group when there is no restore, reuse, or later-sibling move.
- [ ] Constrain `find_later_sibling` to inspect only siblings of the current parent.
- [ ] Ensure `find_later_sibling` never scans into grandchildren.
- [ ] Ensure `find_later_sibling` never scans beyond the current parent range.
- [ ] Ensure `find_later_sibling` never searches retained or detached groups.
- [ ] Remove any fixed rescue-search budget from sibling lookup.
- [ ] Use a linear parent-bounded scan for small sibling counts.
- [ ] Build a lazy per-frame `SiblingIndex` for larger sibling ranges.
- [ ] Build `SiblingIndex` from `[next_child, old_end)` and include only direct children.
- [ ] Implement `finish_group_body` to detach unvisited old children.
- [ ] Implement `finish_group_body` to close payloads for the current group.
- [ ] Implement `finish_group_body` to close nodes for the current group.
- [ ] Implement `finish_group_body` to recompute exact sizes.
- [ ] Implement `finish_group_body` to return detached children to the composer without making retention decisions.
- [ ] Implement `end_group` to pop the current frame.
- [ ] Implement `end_group` to advance the parent cursor past the closed group.

## Detach And Restore

- [ ] Implement `DetachedSubtree { root_key, groups, payloads, nodes, root_nodes, scope_ids, anchors, generation }`.
- [ ] Make detached subtrees own remembered payloads.
- [ ] Ensure dropping a detached subtree disposes remembered values and effect state.
- [ ] Implement `detach_subtree`.
- [ ] Drain the exact group range for detach using `subtree_len`.
- [ ] Extract payloads for detached groups during detach.
- [ ] Extract nodes for detached groups during detach.
- [ ] Mark detached anchors as detached during detach.
- [ ] Mark detached scopes as detached during detach.
- [ ] Repair indices after subtree removal.
- [ ] Implement `restore_detached_at_cursor`.
- [ ] Reparent the restored root to the current parent during restore.
- [ ] Insert restored groups at the writer cursor.
- [ ] Insert restored payloads during restore.
- [ ] Insert restored nodes during restore.
- [ ] Mark restored anchors as active during restore.
- [ ] Mark restored scopes as active during restore.
- [ ] Repair indices after subtree insertion.
- [ ] Keep storage responsible for restoring records only.
- [ ] Keep the composer responsible for scope reactivation and node reattachment after restore.

## Composer-Owned Retention

- [ ] Create `crates/cranpose-core/src/retention.rs`.
- [ ] Implement `RetentionMode::DisposeWhenInactive`.
- [ ] Implement `RetentionMode::RetainWhenInactive`.
- [ ] Implement `RetainKey { parent_scope, key }`.
- [ ] Implement `RetainedGroup { retain_key, subtree, dirty, retained_nodes, scope_ids }`.
- [ ] Implement `RetentionManager { groups, nodes }`.
- [ ] Add `retention: RefCell<RetentionManager>` to `ComposerCore`.
- [ ] Add `scope_registry: RefCell<HashMap<ScopeId, RecomposeScope>>` to `ComposerCore`.
- [ ] Add `current_group_options: RefCell<Vec<GroupOptionsFrame>>` to `ComposerCore`.
- [ ] Extend `RecomposeOptions` with `retention: RetentionMode`.
- [ ] Keep `RecomposeOptions::default()` set to `RetentionMode::DisposeWhenInactive`.
- [ ] Make `cranpose_with_reuse` default to `RetentionMode::RetainWhenInactive` unless explicitly overridden.
- [ ] Rewrite `Composer::with_group` around V2 group semantics.
- [ ] Compute the group key in `with_group`.
- [ ] Compute the retain key in `with_group`.
- [ ] Take a retained subtree from the retention manager when present.
- [ ] Call `begin_group` with an optional restored subtree.
- [ ] Obtain or create the `RecomposeScope` for the started group.
- [ ] Register the scope ID in `scope_registry`.
- [ ] Write the scope ID back into slot storage with `set_group_scope`.
- [ ] Apply `force_recompose`, `force_reuse`, and `Restored` behavior when preparing the scope.
- [ ] Run the group body under scope observation.
- [ ] Call `finish_group_body` after executing the body.
- [ ] Retain or dispose returned detached children according to retention mode.
- [ ] Pop the scope after body completion.
- [ ] Mark the scope recomposed.
- [ ] End the group in slot storage.
- [ ] When a retained inactive scope is invalidated, do not attempt slot-table recomposition by anchor.
- [ ] When a retained inactive scope is invalidated, mark the retained group dirty.
- [ ] When a retained inactive scope is invalidated, optionally schedule the nearest active ancestor or root.
- [ ] When a retained group is restored, call `scope.reactivate()`.
- [ ] When a retained group is restored, call `scope.force_recompose()`.
- [ ] Keep the current rule that inactive scopes mark invalid but do not enqueue active recomposition immediately.

## Recomposition Entry

- [ ] Implement `ScopeIndex { active, detached }`.
- [ ] Resolve active recomposition through `ScopeIndex` instead of scanning the table.
- [ ] Implement `begin_recompose_at_scope` by loading the active anchor from the scope index.
- [ ] Resolve the active group through the anchor registry in `begin_recompose_at_scope`.
- [ ] Start writer recomposition from the resolved group.
- [ ] Return `None` from storage for detached scopes.
- [ ] Make runtime or composer check the retention manager for detached scope recomposition.
- [ ] Mark retained detached entries dirty instead of trying to recompose them through active storage.
- [ ] Force recomposition later when the retained subtree is restored.

## Node Lifecycle

- [ ] Introduce `NodeLifecycle::Active`.
- [ ] Introduce `NodeLifecycle::RetainedDetached`.
- [ ] Introduce `NodeLifecycle::Disposed`.
- [ ] Make `record_node(id)` record a node under the current group without overwriting a slot.
- [ ] Make `record_node(id)` not require `peek_node`.
- [ ] Make `record_node(id)` not require `step_back`.
- [ ] Return `NodeRecordResult { reused, id }` from `record_node`.
- [ ] When retaining a removed group, remove its root node IDs from parent child lists.
- [ ] When retaining a removed group, do not call `applier.remove(node_id)`.
- [ ] When retaining a removed group, optionally call `on_removed_from_parent` and `unmount` if detached nodes must become inactive for the renderer.
- [ ] When retaining a removed group, mark its node IDs as retained in `RetentionManager`.
- [ ] When restoring a retained group, record the existing node IDs again under the restored group.
- [ ] When restoring a retained group, let parent diff reattach those existing node IDs.
- [ ] When restoring a retained group, avoid creating new nodes unless the composable emits a genuinely different node type or key.
- [ ] When disposing a removed group, remove each node from its parent.
- [ ] When disposing a removed group, unmount each node.
- [ ] When disposing a removed group, call `applier.remove(node_id)`.
- [ ] When disposing a removed group, allow payload and effect drops.
- [ ] Update parent diff so retained children are detached without removing applier nodes.
- [ ] Update parent diff so non-retained children are removed and disposed.
- [ ] Ensure retaining payloads while deleting nodes is impossible.

## Keying Model

- [ ] Make ordinary composable calls use `GroupKey { static_key: location_key(...), explicit_key: None, ordinal }`.
- [ ] Use sibling ordinal to disambiguate duplicate unkeyed sibling calls with the same static key.
- [ ] Make explicit keyed content use `GroupKey { static_key: location_key(...), explicit_key: Some(hash_key(user_key)), ordinal: 0 }`.
- [ ] Make item identity be source-location plus user key instead of only the user key.
- [ ] Accept `u64` collision risk initially because current Cranpose already hashes to `u64`.
- [ ] Leave room for future `Key128` or stronger key models.
- [ ] Add optional debug collision info using source file, line, and column metadata.

## Backend Strategy

- [ ] Choose backend strategy Option A by simplifying to `SlotBackendKind::Default` and `SlotBackend::Default(SlotTable)`, or choose Option B by preserving the enum surface but routing every backend kind to V2 `SlotTable`.
- [ ] If Option B is chosen, make `SlotBackend::new(_kind)` always construct the V2 table backend.
- [ ] Do not preserve old backend internals.

## Invariants And Validation

- [ ] Implement `SlotTable::validate()`.
- [ ] Call `validate()` in debug tests after each operation.
- [ ] Validate that every active group index is valid.
- [ ] Validate that root groups have `parent = None`.
- [ ] Validate that child groups point to a parent whose range contains them.
- [ ] Validate that `subtree_len` exactly spans descendants in preorder.
- [ ] Validate that sibling groups are contiguous inside their parent range.
- [ ] Validate that no child range overlaps another child range.
- [ ] Validate that payload ranges stay within `PayloadTable`.
- [ ] Validate that payload owner anchors resolve to active groups.
- [ ] Validate that node ranges stay within `NodeTable`.
- [ ] Validate that node owner anchors resolve to active groups.
- [ ] Validate that the scope index maps every active `scope_id` to the correct group anchor.
- [ ] Validate that the anchor registry resolves active anchors and rejects invalidated anchors.
- [ ] Validate that detached subtrees are not present in active `groups`.
- [ ] Validate that writer stack frames are balanced and within active group ranges.
- [ ] Validate that no two active groups under the same parent share the same full `GroupKey` unless their ordinal differs.
- [ ] Validate that retained subtree roots have no active parent.
- [ ] Return structured `SlotInvariantError::InvalidParent`.
- [ ] Return structured `SlotInvariantError::BadSubtreeLen`.
- [ ] Return structured `SlotInvariantError::PayloadOutOfRange`.
- [ ] Return structured `SlotInvariantError::ScopeIndexMismatch`.
- [ ] Return structured `SlotInvariantError::AnchorMismatch`.
- [ ] Return structured `SlotInvariantError::WriterFrameOutOfBounds`.
- [ ] Return structured `SlotInvariantError::DuplicateSiblingKey`.

## Unit Tests For Slot Storage

- [ ] Add `crates/cranpose-core/src/slot/tests.rs`.
- [ ] Test that an empty table validates.
- [ ] Test that first composition inserts root, group, value, and node.
- [ ] Test that a second identical composition reuses groups and values.
- [ ] Test that removing a conditional child returns `DetachedSubtree`.
- [ ] Test that a default-removed child is disposed when the composer chooses dispose.
- [ ] Test that a retained child restores its remembered value.
- [ ] Test that a restored child gets `GroupStartKind::Restored`.
- [ ] Test that keyed sibling reorder preserves values.
- [ ] Test that unkeyed sibling ordinal preserves positional order semantics.
- [ ] Test that duplicate explicit keys under the same parent either debug-fail or receive deterministic ordinal handling.
- [ ] Test that nested detach and restore preserve nested payloads and scopes.
- [ ] Test that active scope lookup behaves as O(1) indexed lookup rather than scan-dependent behavior.
- [ ] Test that a detached scope is not recomposed through active-table entry.
- [ ] Test that moving a group updates anchors.
- [ ] Test that deleting a group invalidates anchors.
- [ ] Test that value type mismatch panics or returns a typed error consistently.
- [ ] Test that `skip_group` advances by exact subtree size.
- [ ] Test that the node list for a skipped group is exact and stable.
- [ ] Test that retained nodes are not disposed.
- [ ] Test that disposed nodes are removed.

## Composer Integration Tests

- [ ] Test that `remember` survives normal recomposition.
- [ ] Test that `remember` resets when a conditional branch disappears with default retention.
- [ ] Test that `remember` survives when a branch uses retain or reuse options.
- [ ] Test that switching tabs preserves tab state only when retention is requested.
- [ ] Test that switching tabs disposes state when retention is not requested.
- [ ] Test that list item reorder with explicit keys preserves item state.
- [ ] Test that list item reorder without explicit keys follows positional identity.
- [ ] Test that invalidating an active scope recomposes that scope.
- [ ] Test that invalidating an inactive retained scope marks it dirty and recomposes on restore.
- [ ] Test that `DisposableEffect` cleanup runs on dispose but not on retain.
- [ ] Test that retained node IDs are reused on restore.
- [ ] Test that disposed node IDs are not reused unless the applier explicitly reuses IDs.
- [ ] Test that subcompose keeps per-slot compositions and works with V2 storage.

## Property And Model Tests

- [ ] Add a model-test module driven by generated operations.
- [ ] Cover operations for begin group.
- [ ] Cover operations for end group.
- [ ] Cover operations for remember value.
- [ ] Cover operations for record node.
- [ ] Cover operations for conditional include and exclude.
- [ ] Cover operations for keyed sibling moves.
- [ ] Cover operations for retain child.
- [ ] Cover operations for dispose child.
- [ ] Cover operations for restore retained child.
- [ ] Cover operations for skip group.
- [ ] Compare `SlotTable` behavior against a simple reference tree model.
- [ ] Assert that the active tree equals the model tree.
- [ ] Assert that remembered values appear under the same retained identity in the model and implementation.
- [ ] Assert that there are no duplicate active anchors.
- [ ] Assert that no active group also exists in the retention manager.
- [ ] Assert that all invariants hold after every operation.
- [ ] Use `proptest` if acceptable, or deterministic random tests with a fixed seed otherwise.

## Phase 0 - Prepare Branch And Safety Net

- [ ] Create a rewrite branch.
- [ ] Run the current tests to capture baseline failures and pass count.
- [ ] Add `docs/slot_table_v2.md` as a copy of the design.
- [ ] Add a failing placeholder test `slot_v2_empty_table_validates`.

## Phase 1 - Define V2 Types

- [ ] Create `src/slot/mod.rs`.
- [ ] Create `src/slot/types.rs`.
- [ ] Create `src/slot_storage.rs`.
- [ ] Implement `GroupKey`.
- [ ] Implement `GroupId`.
- [ ] Implement `ValueSlotId`.
- [ ] Implement `GroupAnchor`.
- [ ] Implement `GroupStartKind`.
- [ ] Implement `BeginGroupInput`.
- [ ] Implement `GroupStart`.
- [ ] Implement `FinishGroupResult`.
- [ ] Implement `DetachedSubtree`.
- [ ] Implement `SlotInvariantError`.
- [ ] Implement the V2 `SlotStorage` trait.
- [ ] Do not implement old trait compatibility.

## Phase 2 - Implement Core Tables

- [ ] Create `src/slot/table.rs`.
- [ ] Create `src/slot/groups.rs`.
- [ ] Create `src/slot/payload.rs`.
- [ ] Create `src/slot/nodes.rs`.
- [ ] Create `src/slot/anchors.rs`.
- [ ] Create `src/slot/scope_index.rs`.
- [ ] Create `src/slot/validate.rs`.
- [ ] Implement `SlotTable::new`.
- [ ] Implement root and child group insertion.
- [ ] Implement value slots.
- [ ] Implement node records.
- [ ] Implement exact `subtree_len`.
- [ ] Implement validation.
- [ ] Make the empty-table validation test pass.
- [ ] Make the simple group, value, and node composition test pass.

## Phase 3 - Implement Writer Traversal

- [ ] Create `src/slot/writer.rs`.
- [ ] Implement the writer stack.
- [ ] Implement `begin_group` insert and reuse.
- [ ] Implement `end_group`.
- [ ] Implement `finish_group_body` without retention.
- [ ] Implement `skip_group`.
- [ ] Make identical recomposition reuse values.
- [ ] Make skipping advance exactly.
- [ ] Make child removal return a detached subtree.

## Phase 4 - Implement Sibling Moves

- [ ] Implement parent-bounded direct-child scan.
- [ ] Implement lazy `SiblingIndex` for larger sibling ranges.
- [ ] Implement `move_subtree` using `Vec::splice` or drain and insert.
- [ ] Implement anchor repair after subtree moves.
- [ ] Make keyed sibling reorder preserve state.
- [ ] Ensure nested children are not searched or moved as siblings.
- [ ] Make anchors survive sibling moves.

## Phase 5 - Implement Detach And Restore

- [ ] Implement `detach_subtree`.
- [ ] Implement `detach_range`.
- [ ] Implement `restore_detached_at_cursor`.
- [ ] Implement payload extraction and insertion.
- [ ] Implement node extraction and insertion.
- [ ] Implement scope active and detached index updates.
- [ ] Implement anchor active, detached, and invalidated states.
- [ ] Make conditional removal return a valid detached subtree.
- [ ] Make restore recreate the exact active subtree.
- [ ] Make nested restore preserve payloads.
- [ ] Make disposed subtree drop payloads and invalidate anchors.

## Phase 6 - Composer Retention Manager

- [ ] Create `src/retention.rs`.
- [ ] Implement `RetentionMode`.
- [ ] Implement `RetainKey`.
- [ ] Implement `RetainedGroup`.
- [ ] Implement `RetentionManager`.
- [ ] Implement retained node tracking.
- [ ] Implement dirty retained scope tracking.
- [ ] Add `retention` to `ComposerCore`.
- [ ] Add `scope_registry` to `ComposerCore`.
- [ ] Update `pending_scope_options` to include retention mode.
- [ ] Make default conditional branch disposal pass.
- [ ] Make retain mode preserve remembered state.
- [ ] Make invalid retained scope recompose when restored.

## Phase 7 - Update `Composer::with_group`

- [ ] Rewrite `with_group` around V2 semantics.
- [ ] Compute the group key.
- [ ] Compute the retain key.
- [ ] Take retained subtree if present.
- [ ] Call `begin_group` with an optional restored subtree.
- [ ] Obtain or create the remembered `RecomposeScope`.
- [ ] Register the scope ID.
- [ ] Apply `force_recompose`, `force_reuse`, and `Restored` behavior.
- [ ] Run the body under observer.
- [ ] Call `finish_group_body`.
- [ ] Retain or dispose returned children.
- [ ] Pop the scope.
- [ ] End the group.
- [ ] Delete all use of `restored_from_gap`.

## Phase 8 - Update Node And Apply Logic

- [ ] Update parent diff and removal logic for retained nodes.
- [ ] Ensure retained nodes are detached instead of removed from the applier.
- [ ] Ensure disposed nodes are removed.
- [ ] Ensure restored retained nodes can be reattached by existing parent diff.
- [ ] Make retained tab nodes avoid recreation.
- [ ] Make disposed conditional nodes get removed.
- [ ] Make moving keyed nodes reorder rather than destroy.

## Phase 9 - Update Subcompose

- [ ] Keep the existing architectural idea of per-slot compositions and policy-owned reuse.
- [ ] Update `slot_compositions` to use V2 `SlotTable`.
- [ ] Update active and reusable slot registration to mark V2 retention where needed.
- [ ] Update cleanup to dispose compositions that are not active or reusable.
- [ ] Make subcompose basic tests pass.
- [ ] Make lazy list scroll reuse tests pass.
- [ ] Make content-type-compatible reuse tests pass.
- [ ] Make precompose activation tests pass.

## Phase 10 - Remove Old Implementation

- [ ] Remove `Slot::Gap`.
- [ ] Remove `last_start_was_gap`.
- [ ] Remove `has_gap_children`.
- [ ] Remove `mark_range_as_gaps`.
- [ ] Remove old `trim_to_cursor` behavior.
- [ ] Remove `SEARCH_BUDGET`.
- [ ] Remove `EXTENDED_SEARCH_BUDGET`.
- [ ] Remove `SHRINK_MIN_DROP`.
- [ ] Remove `SHRINK_RATIO`.
- [ ] Remove `force_gap_here`.
- [ ] Remove `ensure_gap_at_local`.
- [ ] Remove `find_right_gap_run`.
- [ ] Remove `step_back`.
- [ ] Remove `advance_after_node_read`.
- [ ] Search the repo for the removed names and delete all remaining uses.

## Phase 11 - Documentation And Debug Tools

- [ ] Add `SlotTable::debug_snapshot()`.
- [ ] Add `SlotDebugSnapshot` with active groups, retained counts, anchors, and scopes.
- [ ] Add `COMPOSE_DEBUG_SLOT_TABLE=1` dump support.
- [ ] Add documentation explaining retention versus disposal.

## Phase 12 - Performance Pass

- [ ] Only begin the performance pass after correctness tests are green.
- [ ] Reduce unnecessary `Vec` cloning.
- [ ] Use `SmallVec` for small node lists and payload lists.
- [ ] Build the sibling index lazily.
- [ ] Benchmark keyed list reorder.
- [ ] Benchmark tab switching.
- [ ] Benchmark subcompose scrolling.
- [ ] Consider chunked group storage if large `Vec::splice` costs show up hot.
- [ ] Do not reintroduce semantic gaps for performance.

## Success Criteria

- [ ] There is no `Slot::Gap` equivalent carrying group metadata.
- [ ] There is no `restored_from_gap` API.
- [ ] There are no rescue scan budgets.
- [ ] Group sizes are exact active subtree sizes.
- [ ] Scope lookup is indexed.
- [ ] Removed children are returned as `DetachedSubtree` objects.
- [ ] The composer decides retain versus dispose.
- [ ] Retained state survives restore.
- [ ] Default removed state disposes cleanly.
- [ ] Keyed reorder preserves state and nodes.
- [ ] Unkeyed reorder follows positional semantics.
- [ ] Subcompose still works with per-slot compositions.
- [ ] Debug `validate()` passes after every core test operation.
- [ ] The design is explainable without "gap children", "preserved physical extent", or "extended rescue search" special cases.

## Hard Rules

- [ ] Do not patch the current gap algorithm.
- [ ] Do not preserve group metadata in free storage.
- [ ] Do not scan the whole table to find a group by key.
- [ ] Do not scan the whole table to find a scope.
- [ ] Do not keep stale group lengths.
- [ ] Do not expose cursor repair APIs.
- [ ] Do not make retention automatic for every conditional branch.
- [ ] Do not dispose retained nodes.
- [ ] Do not retain disposed payloads.
- [ ] Do not optimize by weakening invariants.

## Minimal First Passing Implementation

- [ ] Allow plain `Vec<GroupRecord>`.
- [ ] Allow plain `Vec<PayloadRecord>`.
- [ ] Allow subtree movement by drain and insert.
- [ ] Allow O(number of direct siblings) keyed search.
- [ ] Allow no chunking in the first passing version.
- [ ] Allow no packed arrays in the first passing version.
- [ ] Allow no custom allocator in the first passing version.
- [ ] Keep validation everywhere in tests.
- [ ] Do not allow semantic gaps.
- [ ] Do not allow stale subtree lengths.
- [ ] Do not allow global key rescue scans.
- [ ] Do not allow storage-owned retention decisions.

## Future Optimizations

- [ ] Replace direct `Vec` subtree moves with a chunked sequence.
- [ ] Pack `GroupRecord` fields into compact arrays.
- [ ] Use gap-relative anchors internally while keeping semantics unchanged.
- [ ] Add retained subtree LRU limits.
- [ ] Add debug instrumentation for retained memory.
- [ ] Add collision-resistant keys in debug and profile builds.
- [ ] Add specialized lazy-list retention policy.
- [ ] Support allocator-backed tables for no-std if still desired.
