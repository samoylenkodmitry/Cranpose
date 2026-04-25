use super::{
    GroupRecord, SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup,
    SlotDebugScope, SlotDebugSnapshot, SlotTable, SlotTableDebugStats,
};
use crate::{Key, ScopeId};
use std::mem;

impl SlotTable {
    fn group_heap_bytes(&self) -> usize {
        self.groups.capacity() * mem::size_of::<GroupRecord>()
    }

    pub fn heap_bytes(&self) -> usize {
        self.group_heap_bytes()
            + self.payload_heap_bytes()
            + self.node_heap_bytes()
            + self.anchors.heap_bytes()
            + self.payload_locations.capacity() * mem::size_of::<Option<(crate::AnchorId, usize)>>()
            + self.scope_anchor_to_group.capacity() * mem::size_of::<(ScopeId, crate::AnchorId)>()
    }

    pub fn debug_stats(&self) -> SlotTableDebugStats {
        SlotTableDebugStats {
            group_count: self.groups.len(),
            group_capacity: self.groups.capacity(),
            group_record_size: mem::size_of::<GroupRecord>(),
            group_heap_bytes: self.group_heap_bytes(),
            payload_count: self.total_payload_count(),
            payload_capacity: self.payload_debug_capacity(),
            payload_location_count: self.payload_locations.len(),
            payload_location_capacity: self.payload_locations.capacity(),
            node_count: self.total_node_count(),
            node_capacity: self.node_debug_capacity(),
            active_anchor_count: self.anchors.active_len(),
            anchor_slot_count: self.anchors.slot_len(),
            anchor_sparse_count: self.anchors.sparse_slot_len(),
            detached_anchor_count: self.anchors.detached_len(),
            invalidated_anchor_count: self.anchors.invalidated_len(),
            free_anchor_count: self.anchors.free_len(),
            anchor_capacity: self.anchors.capacity(),
            anchor_heap_bytes: self.anchors.heap_bytes(),
            scope_index_count: self.scope_anchor_to_group.len(),
            scope_index_capacity: self.scope_anchor_to_group.capacity(),
            mutation: self.mutation_debug_stats,
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

    pub fn debug_dump_slot_entries(&self) -> Vec<SlotDebugEntry> {
        let mut rows = Vec::new();
        for (index, group) in self.groups.iter().enumerate() {
            rows.push(SlotDebugEntry {
                kind: SlotDebugEntryKind::Group,
                path: format!("group[{index}]"),
                line: format!(
                    "Group(key={:?}, scope={:?}, subtree_len={}, payload_len={}, node_len={})",
                    group.key,
                    group.scope_id,
                    group.subtree_len,
                    self.group_payload_len_at(index),
                    self.group_node_len_at(index)
                ),
            });
        }
        for (group_index, _) in self.groups.iter().enumerate() {
            for (payload_index, payload) in self
                .group_payload_records_at(group_index)
                .iter()
                .enumerate()
            {
                rows.push(SlotDebugEntry {
                    kind: SlotDebugEntryKind::Payload,
                    path: format!("group[{group_index}].payload[{payload_index}]"),
                    line: format!(
                        "Payload(owner={:?}, kind={}, type={})",
                        payload.owner,
                        payload.kind.label(),
                        payload.type_name
                    ),
                });
            }
        }
        for (group_index, _) in self.groups.iter().enumerate() {
            for (node_index, node) in self.group_node_records_at(group_index).iter().enumerate() {
                rows.push(SlotDebugEntry {
                    kind: SlotDebugEntryKind::Node,
                    path: format!("group[{group_index}].node[{node_index}]"),
                    line: format!(
                        "Node(owner={:?}, parent={:?}, id={}, gen={}, lifecycle={:?})",
                        node.owner, node.parent_id, node.id, node.generation, node.lifecycle
                    ),
                });
            }
        }
        rows
    }
}
