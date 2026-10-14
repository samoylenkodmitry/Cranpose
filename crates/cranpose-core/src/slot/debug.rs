use crate::{AnchorId, Key, ScopeId};

/// Size and count figures for one slot table, from `SlotTable::debug_stats`.
/// Counts are in items and sizes are in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableLocalDebugStats {
    /// Groups stored in the table.
    pub group_count: usize,
    /// Groups the group vector holds before it reallocates.
    pub group_capacity: usize,
    /// Size of one group record in bytes.
    pub group_record_size: usize,
    /// Heap bytes reserved for group records.
    pub group_heap_bytes: usize,
    /// Payload slots stored across all groups.
    pub payload_count: usize,
    /// Payload slots the payload store holds before it reallocates.
    pub payload_capacity: usize,
    /// Payload anchors that point at a live payload.
    pub active_payload_anchor_count: usize,
    /// Entries in the payload anchor registry in any state.
    pub payload_anchor_slot_count: usize,
    /// Payload anchors whose payload is detached from the table.
    pub detached_payload_anchor_count: usize,
    /// Payload anchor entries that no longer resolve to a payload.
    pub invalidated_payload_anchor_count: usize,
    /// Payload anchor ids free for reuse.
    pub free_payload_anchor_count: usize,
    /// Payload anchor entries the registry holds before it reallocates.
    pub payload_anchor_capacity: usize,
    /// Heap bytes reserved by the payload anchor registry.
    pub payload_anchor_heap_bytes: usize,
    /// Nodes stored across all groups.
    pub node_count: usize,
    /// Nodes the node storage holds before it reallocates.
    pub node_capacity: usize,
    /// Group anchors that point at a live group.
    pub active_anchor_count: usize,
    /// Entries in the group anchor registry in any state.
    pub anchor_slot_count: usize,
    /// Anchor entries kept in the sparse map instead of the dense vector.
    pub anchor_sparse_count: usize,
    /// Group anchors whose group is detached from the table.
    pub detached_anchor_count: usize,
    /// Group anchor entries that no longer resolve to a group.
    pub invalidated_anchor_count: usize,
    /// Group anchor ids free for reuse.
    pub free_anchor_count: usize,
    /// Anchor entries the registry holds before it reallocates.
    pub anchor_capacity: usize,
    /// Heap bytes reserved by the group anchor registry.
    pub anchor_heap_bytes: usize,
    /// Counters for subtree moves and index refreshes.
    pub mutation: SlotTableMutationDebugStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SlotLifecycleDebugStats {
    pub pending_drop_count: usize,
    pub pending_drop_capacity: usize,
}

/// Counters for subtrees the composer keeps for hidden content, from the retention state.
/// Counts are in items and sizes are in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotRetentionDebugStats {
    /// Subtrees currently retained.
    pub retained_subtree_count: usize,
    /// Groups inside retained subtrees.
    pub retained_group_count: usize,
    /// Payload slots inside retained subtrees.
    pub retained_payload_count: usize,
    /// Nodes inside retained subtrees.
    pub retained_node_count: usize,
    /// Recompose scopes inside retained subtrees.
    pub retained_scope_count: usize,
    /// Group anchors held by retained subtrees.
    pub retained_anchor_count: usize,
    /// Heap bytes held by retained subtrees.
    pub retained_heap_bytes: usize,
    /// Subtrees dropped from retention since the composition started.
    pub retained_evictions_total: usize,
}

/// Combined size and count figures for a slot table: the table's own figures, its pending
/// drops and its retained subtrees.
/// Counts are in items and sizes are in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    /// Groups stored in the table.
    pub group_count: usize,
    /// Groups the group vector holds before it reallocates.
    pub group_capacity: usize,
    /// Size of one group record in bytes.
    pub group_record_size: usize,
    /// Heap bytes reserved for group records.
    pub group_heap_bytes: usize,
    /// Payload slots stored across all groups.
    pub payload_count: usize,
    /// Payload slots the payload store holds before it reallocates.
    pub payload_capacity: usize,
    /// Payload anchors that point at a live payload.
    pub active_payload_anchor_count: usize,
    /// Entries in the payload anchor registry in any state.
    pub payload_anchor_slot_count: usize,
    /// Payload anchors whose payload is detached from the table.
    pub detached_payload_anchor_count: usize,
    /// Payload anchor entries that no longer resolve to a payload.
    pub invalidated_payload_anchor_count: usize,
    /// Payload anchor ids free for reuse.
    pub free_payload_anchor_count: usize,
    /// Payload anchor entries the registry holds before it reallocates.
    pub payload_anchor_capacity: usize,
    /// Heap bytes reserved by the payload anchor registry.
    pub payload_anchor_heap_bytes: usize,
    /// Nodes stored across all groups.
    pub node_count: usize,
    /// Nodes the node storage holds before it reallocates.
    pub node_capacity: usize,
    /// Removed values waiting for their deferred drop.
    pub pending_drop_count: usize,
    /// Values the pending drop queue holds before it reallocates.
    pub pending_drop_capacity: usize,
    /// Group anchors that point at a live group.
    pub active_anchor_count: usize,
    /// Entries in the group anchor registry in any state.
    pub anchor_slot_count: usize,
    /// Anchor entries kept in the sparse map instead of the dense vector.
    pub anchor_sparse_count: usize,
    /// Group anchors whose group is detached from the table.
    pub detached_anchor_count: usize,
    /// Group anchor entries that no longer resolve to a group.
    pub invalidated_anchor_count: usize,
    /// Group anchor ids free for reuse.
    pub free_anchor_count: usize,
    /// Anchor entries the registry holds before it reallocates.
    pub anchor_capacity: usize,
    /// Heap bytes reserved by the group anchor registry.
    pub anchor_heap_bytes: usize,
    /// Subtrees currently retained.
    pub retained_subtree_count: usize,
    /// Groups inside retained subtrees.
    pub retained_group_count: usize,
    /// Payload slots inside retained subtrees.
    pub retained_payload_count: usize,
    /// Nodes inside retained subtrees.
    pub retained_node_count: usize,
    /// Recompose scopes inside retained subtrees.
    pub retained_scope_count: usize,
    /// Group anchors held by retained subtrees.
    pub retained_anchor_count: usize,
    /// Heap bytes held by retained subtrees.
    pub retained_heap_bytes: usize,
    /// Subtrees dropped from retention since the composition started.
    pub retained_evictions_total: usize,
    /// Counters for subtree moves and index refreshes.
    pub mutation: SlotTableMutationDebugStats,
}

impl SlotTableDebugStats {
    pub(crate) fn from_parts(
        local: SlotTableLocalDebugStats,
        lifecycle: SlotLifecycleDebugStats,
        retention: SlotRetentionDebugStats,
    ) -> Self {
        Self {
            group_count: local.group_count,
            group_capacity: local.group_capacity,
            group_record_size: local.group_record_size,
            group_heap_bytes: local.group_heap_bytes,
            payload_count: local.payload_count,
            payload_capacity: local.payload_capacity,
            active_payload_anchor_count: local.active_payload_anchor_count,
            payload_anchor_slot_count: local.payload_anchor_slot_count,
            detached_payload_anchor_count: local.detached_payload_anchor_count,
            invalidated_payload_anchor_count: local.invalidated_payload_anchor_count,
            free_payload_anchor_count: local.free_payload_anchor_count,
            payload_anchor_capacity: local.payload_anchor_capacity,
            payload_anchor_heap_bytes: local.payload_anchor_heap_bytes,
            node_count: local.node_count,
            node_capacity: local.node_capacity,
            pending_drop_count: lifecycle.pending_drop_count,
            pending_drop_capacity: lifecycle.pending_drop_capacity,
            active_anchor_count: local.active_anchor_count,
            anchor_slot_count: local.anchor_slot_count,
            anchor_sparse_count: local.anchor_sparse_count,
            detached_anchor_count: local.detached_anchor_count,
            invalidated_anchor_count: local.invalidated_anchor_count,
            free_anchor_count: local.free_anchor_count,
            anchor_capacity: local.anchor_capacity,
            anchor_heap_bytes: local.anchor_heap_bytes,
            retained_subtree_count: retention.retained_subtree_count,
            retained_group_count: retention.retained_group_count,
            retained_payload_count: retention.retained_payload_count,
            retained_node_count: retention.retained_node_count,
            retained_scope_count: retention.retained_scope_count,
            retained_anchor_count: retention.retained_anchor_count,
            retained_heap_bytes: retention.retained_heap_bytes,
            retained_evictions_total: retention.retained_evictions_total,
            mutation: local.mutation,
        }
    }
}

/// Counters for work the slot table does when groups move, from the table's debug stats.
/// `*_count` fields count operations or items summed over all operations; `*_max_span` fields
/// hold the largest single operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableMutationDebugStats {
    /// Subtree moves done.
    pub subtree_move_count: usize,
    /// Groups moved, summed over all subtree moves.
    pub moved_group_count: usize,
    /// Most groups moved by one subtree move.
    pub moved_group_max_span: usize,
    /// Payload slots moved, summed over all subtree moves.
    pub moved_payload_count: usize,
    /// Most payload slots moved by one subtree move.
    pub moved_payload_max_span: usize,
    /// Nodes moved, summed over all subtree moves.
    pub moved_node_count: usize,
    /// Most nodes moved by one subtree move.
    pub moved_node_max_span: usize,
    /// Times the payload locations of a range of groups were rewritten.
    pub payload_location_range_refresh_count: usize,
    /// Groups covered, summed over all range refreshes of payload locations.
    pub payload_location_range_refresh_group_count: usize,
    /// Most groups covered by one range refresh of payload locations.
    pub payload_location_range_refresh_group_max_span: usize,
    /// Payload slots rewritten, summed over all range refreshes of payload locations.
    pub payload_location_range_refresh_payload_count: usize,
    /// Most payload slots rewritten by one range refresh of payload locations.
    pub payload_location_range_refresh_payload_max_span: usize,
    /// Times the payload locations of a single group were rewritten.
    pub payload_location_refresh_count: usize,
    /// Payload slots rewritten, summed over all single-group refreshes of payload locations.
    pub payload_location_refresh_payload_count: usize,
    /// Most payload slots rewritten by one single-group refresh of payload locations.
    pub payload_location_refresh_max_span: usize,
    /// Times the stored group indexes behind the group anchors were rewritten.
    pub group_index_refresh_count: usize,
    /// Groups covered, summed over all group index refreshes.
    pub group_index_refresh_group_count: usize,
    /// Most groups covered by one group index refresh.
    pub group_index_refresh_max_span: usize,
    /// Times the cached payload and node segment ranges were updated.
    pub segment_range_update_count: usize,
    /// Groups covered, summed over all segment range updates.
    pub segment_range_update_group_count: usize,
    /// Most groups covered by one segment range update.
    pub segment_range_update_max_span: usize,
    /// Bytes the payload storage moved to insert, remove, detach, restore or
    /// reorder payloads.
    pub payload_shift_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SlotTableDiagnostics {
    mutation: SlotTableMutationDebugStats,
}

impl SlotTableDiagnostics {
    pub(crate) fn mutation(&self) -> SlotTableMutationDebugStats {
        self.mutation
    }

    pub(crate) fn record_subtree_move(
        &mut self,
        group_span: usize,
        payload_span: usize,
        node_span: usize,
    ) {
        self.mutation
            .record_subtree_move(group_span, payload_span, node_span);
    }

    pub(crate) fn record_payload_location_range_refresh(
        &mut self,
        group_span: usize,
        payload_span: usize,
    ) {
        self.mutation
            .record_payload_location_range_refresh(group_span, payload_span);
    }

    pub(crate) fn record_group_index_refresh(&mut self, group_span: usize) {
        self.mutation.record_group_index_refresh(group_span);
    }

    pub(crate) fn record_payload_location_refresh(&mut self, payload_span: usize) {
        self.mutation.record_payload_location_refresh(payload_span);
    }

    pub(crate) fn record_segment_range_update(&mut self, group_span: usize) {
        self.mutation.record_segment_range_update(group_span);
    }
}

impl SlotTableMutationDebugStats {
    pub(crate) fn record_subtree_move(
        &mut self,
        group_span: usize,
        payload_span: usize,
        node_span: usize,
    ) {
        self.subtree_move_count = self.subtree_move_count.saturating_add(1);
        self.moved_group_count = self.moved_group_count.saturating_add(group_span);
        self.moved_group_max_span = self.moved_group_max_span.max(group_span);
        self.moved_payload_count = self.moved_payload_count.saturating_add(payload_span);
        self.moved_payload_max_span = self.moved_payload_max_span.max(payload_span);
        self.moved_node_count = self.moved_node_count.saturating_add(node_span);
        self.moved_node_max_span = self.moved_node_max_span.max(node_span);
    }

    pub(crate) fn record_payload_location_range_refresh(
        &mut self,
        group_span: usize,
        payload_span: usize,
    ) {
        self.payload_location_range_refresh_count =
            self.payload_location_range_refresh_count.saturating_add(1);
        self.payload_location_range_refresh_group_count = self
            .payload_location_range_refresh_group_count
            .saturating_add(group_span);
        self.payload_location_range_refresh_group_max_span = self
            .payload_location_range_refresh_group_max_span
            .max(group_span);
        self.payload_location_range_refresh_payload_count = self
            .payload_location_range_refresh_payload_count
            .saturating_add(payload_span);
        self.payload_location_range_refresh_payload_max_span = self
            .payload_location_range_refresh_payload_max_span
            .max(payload_span);
    }

    pub(crate) fn record_group_index_refresh(&mut self, group_span: usize) {
        self.group_index_refresh_count = self.group_index_refresh_count.saturating_add(1);
        self.group_index_refresh_group_count = self
            .group_index_refresh_group_count
            .saturating_add(group_span);
        self.group_index_refresh_max_span = self.group_index_refresh_max_span.max(group_span);
    }

    pub(crate) fn record_payload_location_refresh(&mut self, payload_span: usize) {
        self.payload_location_refresh_count = self.payload_location_refresh_count.saturating_add(1);
        self.payload_location_refresh_payload_count = self
            .payload_location_refresh_payload_count
            .saturating_add(payload_span);
        self.payload_location_refresh_max_span =
            self.payload_location_refresh_max_span.max(payload_span);
    }

    pub(crate) fn record_segment_range_update(&mut self, group_span: usize) {
        self.segment_range_update_count = self.segment_range_update_count.saturating_add(1);
        self.segment_range_update_group_count = self
            .segment_range_update_group_count
            .saturating_add(group_span);
        self.segment_range_update_max_span = self.segment_range_update_max_span.max(group_span);
    }
}

/// The kind of row in the slot table dump returned by `SlotTable::debug_dump_slot_entries`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotDebugEntryKind {
    /// A group row.
    Group,
    /// A payload row, such as a remembered value.
    Payload,
    /// A node row.
    Node,
}

/// One row in the slot table dump returned by `SlotTable::debug_dump_slot_entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotDebugEntry {
    /// What kind of item the row describes.
    pub kind: SlotDebugEntryKind,
    /// Location of the item in the table, such as `group[2].payload[0]`.
    pub path: String,
    /// Text description of the item for logs.
    pub line: String,
}

/// A copy of the groups, anchors and scopes of a slot table, for tests and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotDebugSnapshot {
    /// Every group in the table, in table order.
    pub active_groups: Vec<SlotDebugGroup>,
    /// Every live group anchor with the index of the group it points at, sorted by that index.
    pub anchors: Vec<SlotDebugAnchor>,
    /// Every recompose scope with its group, sorted by scope id.
    pub scopes: Vec<SlotDebugScope>,
    /// Payload slots in the table.
    pub active_payload_count: usize,
    /// Nodes in the table.
    pub active_node_count: usize,
    /// Groups that own a recompose scope.
    pub active_scope_count: usize,
    /// Subtrees retained for hidden content.
    pub retained_subtree_count: usize,
    /// Groups inside retained subtrees.
    pub retained_group_count: usize,
    /// Payload slots inside retained subtrees.
    pub retained_payload_count: usize,
    /// Nodes inside retained subtrees.
    pub retained_node_count: usize,
    /// Recompose scopes inside retained subtrees.
    pub retained_scope_count: usize,
}

/// One group of a slot table as seen in a [`SlotDebugSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugGroup {
    /// Position of the group in the table's group order.
    pub index: usize,
    /// Anchor that identifies this group.
    pub anchor: AnchorId,
    /// Anchor of the parent group.
    pub parent_anchor: AnchorId,
    /// Key of the call site that produced the group.
    pub static_key: Key,
    /// Key the caller gave the group, if any.
    pub explicit_key: Option<Key>,
    /// Position of the group among earlier sibling groups with the same call site key. A
    /// keyed group has ordinal 0.
    pub ordinal: u32,
    /// Id of the recompose scope the group owns, if any.
    pub scope_id: Option<ScopeId>,
    /// Nesting depth of the group; top level groups have the lowest depth.
    pub depth: u32,
    /// Number of groups in the group's subtree, including the group itself.
    pub subtree_len: u32,
    /// Payload slots the group owns directly.
    pub payload_len: usize,
    /// Nodes the group owns directly.
    pub node_len: usize,
    /// Nodes in the group's subtree.
    pub subtree_node_count: u32,
}

/// A live group anchor and the group it points at, in a [`SlotDebugSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugAnchor {
    /// The anchor.
    pub anchor: AnchorId,
    /// Index of the group the anchor points at.
    pub group_index: usize,
}

/// A recompose scope and the group that owns it, in a [`SlotDebugSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugScope {
    /// Id of the recompose scope.
    pub scope_id: ScopeId,
    /// Anchor of the group that owns the scope.
    pub anchor: AnchorId,
    /// Index of the group that owns the scope.
    pub group_index: usize,
}
