use crate::{AnchorId, Key, ScopeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableLocalDebugStats {
    pub group_count: usize,
    pub group_capacity: usize,
    pub group_record_size: usize,
    pub group_heap_bytes: usize,
    pub payload_count: usize,
    pub payload_capacity: usize,
    pub active_payload_anchor_count: usize,
    pub payload_anchor_slot_count: usize,
    pub detached_payload_anchor_count: usize,
    pub invalidated_payload_anchor_count: usize,
    pub free_payload_anchor_count: usize,
    pub payload_anchor_capacity: usize,
    pub payload_anchor_heap_bytes: usize,
    pub node_count: usize,
    pub node_capacity: usize,
    pub active_anchor_count: usize,
    pub anchor_slot_count: usize,
    pub anchor_sparse_count: usize,
    pub detached_anchor_count: usize,
    pub invalidated_anchor_count: usize,
    pub free_anchor_count: usize,
    pub anchor_capacity: usize,
    pub anchor_heap_bytes: usize,
    pub scope_index_count: usize,
    pub scope_index_capacity: usize,
    pub mutation: SlotTableMutationDebugStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SlotLifecycleDebugStats {
    pub pending_drop_count: usize,
    pub pending_drop_capacity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotRetentionDebugStats {
    pub retained_subtree_count: usize,
    pub retained_group_count: usize,
    pub retained_payload_count: usize,
    pub retained_node_count: usize,
    pub retained_scope_count: usize,
    pub retained_anchor_count: usize,
    pub retained_heap_bytes: usize,
    pub retained_evictions_total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub group_count: usize,
    pub group_capacity: usize,
    pub group_record_size: usize,
    pub group_heap_bytes: usize,
    pub payload_count: usize,
    pub payload_capacity: usize,
    pub active_payload_anchor_count: usize,
    pub payload_anchor_slot_count: usize,
    pub detached_payload_anchor_count: usize,
    pub invalidated_payload_anchor_count: usize,
    pub free_payload_anchor_count: usize,
    pub payload_anchor_capacity: usize,
    pub payload_anchor_heap_bytes: usize,
    pub node_count: usize,
    pub node_capacity: usize,
    pub pending_drop_count: usize,
    pub pending_drop_capacity: usize,
    pub active_anchor_count: usize,
    pub anchor_slot_count: usize,
    pub anchor_sparse_count: usize,
    pub detached_anchor_count: usize,
    pub invalidated_anchor_count: usize,
    pub free_anchor_count: usize,
    pub anchor_capacity: usize,
    pub anchor_heap_bytes: usize,
    pub scope_index_count: usize,
    pub scope_index_capacity: usize,
    pub retained_subtree_count: usize,
    pub retained_group_count: usize,
    pub retained_payload_count: usize,
    pub retained_node_count: usize,
    pub retained_scope_count: usize,
    pub retained_anchor_count: usize,
    pub retained_heap_bytes: usize,
    pub retained_evictions_total: usize,
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
            scope_index_count: local.scope_index_count,
            scope_index_capacity: local.scope_index_capacity,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableMutationDebugStats {
    pub subtree_move_count: usize,
    pub moved_group_count: usize,
    pub moved_group_max_span: usize,
    pub moved_payload_count: usize,
    pub moved_payload_max_span: usize,
    pub moved_node_count: usize,
    pub moved_node_max_span: usize,
    pub payload_location_range_refresh_count: usize,
    pub payload_location_range_refresh_group_count: usize,
    pub payload_location_range_refresh_group_max_span: usize,
    pub payload_location_range_refresh_payload_count: usize,
    pub payload_location_range_refresh_payload_max_span: usize,
    pub payload_location_refresh_count: usize,
    pub payload_location_refresh_payload_count: usize,
    pub payload_location_refresh_max_span: usize,
    pub group_index_refresh_count: usize,
    pub group_index_refresh_group_count: usize,
    pub group_index_refresh_max_span: usize,
    pub segment_range_update_count: usize,
    pub segment_range_update_group_count: usize,
    pub segment_range_update_max_span: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotDebugEntryKind {
    Group,
    Payload,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotDebugEntry {
    pub kind: SlotDebugEntryKind,
    pub path: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SlotDebugSnapshot {
    pub active_groups: Vec<SlotDebugGroup>,
    pub anchors: Vec<SlotDebugAnchor>,
    pub scopes: Vec<SlotDebugScope>,
    pub active_payload_count: usize,
    pub active_node_count: usize,
    pub active_scope_count: usize,
    pub scope_index_count: usize,
    pub runtime_scope_registry_count: Option<usize>,
    pub retained_subtree_count: usize,
    pub retained_group_count: usize,
    pub retained_payload_count: usize,
    pub retained_node_count: usize,
    pub retained_scope_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugGroup {
    pub index: usize,
    pub anchor: AnchorId,
    pub parent_anchor: AnchorId,
    pub static_key: Key,
    pub explicit_key: Option<Key>,
    pub ordinal: u32,
    pub scope_id: Option<ScopeId>,
    pub depth: u32,
    pub subtree_len: u32,
    pub payload_len: usize,
    pub node_len: usize,
    pub subtree_node_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugAnchor {
    pub anchor: AnchorId,
    pub group_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDebugScope {
    pub scope_id: ScopeId,
    pub anchor: AnchorId,
    pub group_index: usize,
}
