use super::{
    segments::{
        add_group_segment_len, group_segment_len, group_segment_range_at,
        group_segment_range_checked, group_segment_start, offset_detached_group_segment_starts,
        segment_insert_index_for_group, shift_group_segment_starts_from, subtree_segment_span,
        NodeSegment,
    },
    GroupRecord, NodeLifecycle, NodeRecord, SlotTable,
};
use crate::NodeId;
use std::{mem, ops::Range};

pub(super) struct GroupNodeRecordResult {
    pub(super) reused_slot: bool,
    pub(super) reused_node: bool,
}

impl SlotTable {
    fn group_node_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<NodeSegment>(&self.groups, group_index)
    }

    pub(in crate::slot) fn group_node_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<NodeSegment>(&self.groups, group_index)
    }

    pub(in crate::slot) fn group_node_range_checked(
        &self,
        group_index: usize,
    ) -> Option<Range<usize>> {
        group_segment_range_checked::<NodeSegment>(&self.groups, self.nodes.len(), group_index)
    }

    fn group_node_range_at(&self, group_index: usize) -> Range<usize> {
        group_segment_range_at::<NodeSegment>(&self.groups, self.nodes.len(), group_index)
    }

    pub(in crate::slot) fn group_node_records_at(&self, group_index: usize) -> &[NodeRecord] {
        let range = self.group_node_range_at(group_index);
        &self.nodes[range]
    }

    pub(in crate::slot) fn group_node_record_at(
        &self,
        group_index: usize,
        node_index: usize,
    ) -> &NodeRecord {
        self.group_node_records_at(group_index)
            .get(node_index)
            .expect("node index should resolve")
    }

    pub(in crate::slot) fn group_node_record_at_mut(
        &mut self,
        group_index: usize,
        node_index: usize,
    ) -> &mut NodeRecord {
        let node_start = self.group_node_start_at(group_index);
        self.nodes
            .get_mut(node_start + node_index)
            .expect("node index should resolve")
    }

    pub(in crate::slot) fn total_node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(super) fn node_heap_bytes(&self) -> usize {
        self.nodes.capacity() * mem::size_of::<NodeRecord>()
    }

    pub(super) fn node_debug_capacity(&self) -> usize {
        self.nodes.capacity()
    }

    pub(super) fn record_group_node(
        &mut self,
        group_index: usize,
        node_index: usize,
        owner: crate::AnchorId,
        id: NodeId,
        parent_id: Option<NodeId>,
        generation: u32,
    ) -> GroupNodeRecordResult {
        if node_index < self.group_node_len_at(group_index) {
            let existing = *self.group_node_record_at(group_index, node_index);
            *self.group_node_record_at_mut(group_index, node_index) = NodeRecord {
                owner,
                id,
                parent_id,
                generation,
                lifecycle: NodeLifecycle::Active,
            };
            GroupNodeRecordResult {
                reused_slot: true,
                reused_node: existing.id == id && existing.generation == generation,
            }
        } else {
            let insert_index = segment_insert_index_for_group::<NodeSegment>(
                &self.groups,
                self.nodes.len(),
                group_index,
            ) + node_index;
            self.nodes.insert(
                insert_index,
                NodeRecord {
                    owner,
                    id,
                    parent_id,
                    generation,
                    lifecycle: NodeLifecycle::Active,
                },
            );
            add_group_segment_len::<NodeSegment>(&mut self.groups, group_index, 1);
            shift_group_segment_starts_from::<NodeSegment>(&mut self.groups, group_index + 1, 1);
            GroupNodeRecordResult {
                reused_slot: false,
                reused_node: false,
            }
        }
    }

    pub(super) fn remove_group_node_range(
        &mut self,
        group_index: usize,
        start: usize,
    ) -> Vec<NodeRecord> {
        let node_len = self.group_node_len_at(group_index);
        if start >= node_len {
            return Vec::new();
        }
        let node_start = self.group_node_start_at(group_index) + start;
        let node_end = self.group_node_start_at(group_index) + node_len;
        let removed = self.nodes.drain(node_start..node_end).collect::<Vec<_>>();
        add_group_segment_len::<NodeSegment>(
            &mut self.groups,
            group_index,
            -(removed.len() as i64),
        );
        shift_group_segment_starts_from::<NodeSegment>(
            &mut self.groups,
            group_index + 1,
            -(removed.len() as i64),
        );
        removed
    }

    pub(super) fn detach_nodes_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<NodeRecord> {
        let Some((node_start, node_len)) = subtree_segment_span::<NodeSegment>(removed_groups)
        else {
            return Vec::new();
        };
        offset_detached_group_segment_starts::<NodeSegment>(removed_groups, -(node_start as i64));
        if node_len == 0 {
            return Vec::new();
        }
        let removed = self
            .nodes
            .drain(node_start..node_start + node_len)
            .collect();
        shift_group_segment_starts_from::<NodeSegment>(
            &mut self.groups,
            removed_group_index,
            -(node_len as i64),
        );
        removed
    }

    pub(super) fn restore_nodes_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        nodes: Vec<NodeRecord>,
    ) {
        let node_insert_index = segment_insert_index_for_group::<NodeSegment>(
            &self.groups,
            self.nodes.len(),
            insert_group_index,
        );
        shift_group_segment_starts_from::<NodeSegment>(
            &mut self.groups,
            insert_group_index,
            nodes.len() as i64,
        );
        offset_detached_group_segment_starts::<NodeSegment>(groups, node_insert_index as i64);
        self.nodes
            .splice(node_insert_index..node_insert_index, nodes);
    }
}
