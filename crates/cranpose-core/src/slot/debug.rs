use crate::{AnchorId, Key, ScopeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotTableDebugStats {
    pub slots_len: usize,
    pub slots_cap: usize,
    pub pending_slot_drops_len: usize,
    pub pending_slot_drops_cap: usize,
    pub anchors_len: usize,
    pub anchors_cap: usize,
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
