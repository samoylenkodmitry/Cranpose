use super::{
    GroupRecord, SlotDebugAnchor, SlotDebugGroup, SlotDebugScope, SlotDebugSnapshot, SlotTable,
    SlotTableDebugStats,
};
use crate::{Key, ScopeId};
use std::mem;

impl SlotTable {
    pub fn heap_bytes(&self) -> usize {
        self.groups.capacity() * mem::size_of::<GroupRecord>()
            + self.payload_heap_bytes()
            + self.node_heap_bytes()
            + self.anchors.capacity() * mem::size_of::<(crate::AnchorId, super::AnchorState)>()
            + self.payload_anchor_to_location.capacity()
                * mem::size_of::<(usize, (crate::AnchorId, usize))>()
            + self.scope_anchor_to_group.capacity() * mem::size_of::<(ScopeId, crate::AnchorId)>()
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        let payload_len = self.total_payload_count();
        let payload_cap = self.payload_debug_capacity();
        let node_len = self.total_node_count();
        let node_cap = self.node_debug_capacity();
        SlotTableDebugStats {
            slots_len: self.groups.len() + payload_len + node_len,
            slots_cap: self.groups.capacity() + payload_cap + node_cap,
            anchors_len: self.anchors.active_len(),
            anchors_cap: self.anchors.capacity(),
            ..SlotTableDebugStats::default()
        }
    }

    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                (
                    index,
                    group.key.static_key,
                    group.scope_id,
                    group.subtree_len as usize,
                )
            })
            .collect()
    }

    pub fn debug_snapshot(&self) -> SlotDebugSnapshot {
        let active_groups = self
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| SlotDebugGroup {
                index,
                anchor: group.anchor,
                parent_anchor: group.parent_anchor,
                static_key: group.key.static_key,
                explicit_key: group.key.explicit_key,
                ordinal: group.key.ordinal,
                scope_id: group.scope_id,
                depth: group.depth,
                subtree_len: group.subtree_len,
                payload_len: self.group_payload_len_at(index),
                node_len: self.group_node_len_at(index),
                subtree_node_count: group.subtree_node_count,
            })
            .collect::<Vec<_>>();
        let mut anchors = self
            .anchors
            .active_entries()
            .map(|(anchor, group_index)| SlotDebugAnchor {
                anchor,
                group_index,
            })
            .collect::<Vec<_>>();
        anchors.sort_by_key(|entry| entry.group_index);

        let mut scopes = self
            .scope_anchor_to_group
            .iter()
            .filter_map(|(&scope_id, &anchor)| {
                self.anchors
                    .active_index(anchor)
                    .map(|group_index| SlotDebugScope {
                        scope_id,
                        anchor,
                        group_index,
                    })
            })
            .collect::<Vec<_>>();
        scopes.sort_by_key(|entry| entry.scope_id);

        SlotDebugSnapshot {
            active_payload_count: self.total_payload_count(),
            active_node_count: self.total_node_count(),
            active_scope_count: scopes.len(),
            scope_registry_count: scopes.len(),
            active_groups,
            anchors,
            scopes,
            retained_subtree_count: 0,
            retained_group_count: 0,
            retained_node_count: 0,
            retained_scope_count: 0,
        }
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        let mut rows = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            rows.push((
                index,
                format!(
                    "Group(key={:?}, scope={:?}, subtree_len={}, payload_len={}, node_len={})",
                    group.key,
                    group.scope_id,
                    group.subtree_len,
                    self.group_payload_len_at(index),
                    self.group_node_len_at(index)
                ),
            ));
        }
        let base = rows.len();
        let mut offset = 0usize;
        for (group_index, _) in self.groups.iter().enumerate() {
            for payload in self.group_payload_records_at(group_index) {
                rows.push((
                    base + offset,
                    format!(
                        "Value(owner={:?}, type={})",
                        payload.owner, payload.type_name
                    ),
                ));
                offset += 1;
            }
        }
        let base = rows.len();
        let mut offset = 0usize;
        for (group_index, _) in self.groups.iter().enumerate() {
            for node in self.group_node_records_at(group_index) {
                rows.push((
                    base + offset,
                    format!(
                        "Node(owner={:?}, id={}, gen={}, lifecycle={:?})",
                        node.owner, node.id, node.generation, node.lifecycle
                    ),
                ));
                offset += 1;
            }
        }
        rows
    }
}
