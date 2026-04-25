use crate::{AnchorId, Key, ScopeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub group_count: usize,
    pub group_capacity: usize,
    pub group_record_size: usize,
    pub group_heap_bytes: usize,
    pub payload_count: usize,
    pub payload_capacity: usize,
    pub payload_location_count: usize,
    pub payload_location_capacity: usize,
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
    pub mutation: SlotTableMutationDebugStats,
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
    pub payload_location_rebuild_count: usize,
    pub payload_location_rebuild_group_count: usize,
    pub payload_location_rebuild_group_max_span: usize,
    pub payload_location_rebuild_payload_count: usize,
    pub payload_location_rebuild_payload_max_span: usize,
    pub group_index_refresh_count: usize,
    pub group_index_refresh_group_count: usize,
    pub group_index_refresh_max_span: usize,
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

    pub(crate) fn record_payload_location_rebuild(
        &mut self,
        group_span: usize,
        payload_span: usize,
    ) {
        self.payload_location_rebuild_count = self.payload_location_rebuild_count.saturating_add(1);
        self.payload_location_rebuild_group_count = self
            .payload_location_rebuild_group_count
            .saturating_add(group_span);
        self.payload_location_rebuild_group_max_span =
            self.payload_location_rebuild_group_max_span.max(group_span);
        self.payload_location_rebuild_payload_count = self
            .payload_location_rebuild_payload_count
            .saturating_add(payload_span);
        self.payload_location_rebuild_payload_max_span = self
            .payload_location_rebuild_payload_max_span
            .max(payload_span);
    }

    pub(crate) fn record_group_index_refresh(&mut self, group_span: usize) {
        self.group_index_refresh_count = self.group_index_refresh_count.saturating_add(1);
        self.group_index_refresh_group_count = self
            .group_index_refresh_group_count
            .saturating_add(group_span);
        self.group_index_refresh_max_span = self.group_index_refresh_max_span.max(group_span);
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
    pub scope_registry_count: usize,
    pub retained_subtree_count: usize,
    pub retained_group_count: usize,
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
