use super::{
    checked_usize_to_i64,
    segments::{
        extract_subtree_segment, group_segment_len, group_segment_range_at, group_segment_start,
        group_segment_subrange_at, insert_group_segment_item, remove_group_segment_range,
        restore_subtree_segment, NodeSegment,
    },
    GroupNodeRange, GroupRecord, NodeLifecycle, NodeRange, NodeRecord, NodeSlotUpdate, SlotTable,
};
use crate::{AnchorId, NodeId};
use std::mem;

impl SlotTable {
    fn group_node_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<NodeSegment>(&self.groups, group_index)
    }

    pub(in crate::slot) fn group_node_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<NodeSegment>(&self.groups, group_index)
    }

    fn group_node_range_at(&self, group_index: usize) -> NodeRange {
        group_segment_range_at::<NodeSegment>(&self.groups, self.nodes.len(), group_index)
    }

    pub(in crate::slot) fn group_node_tail_range_at(
        &self,
        group_index: usize,
        start: usize,
    ) -> GroupNodeRange {
        let node_len = self.group_node_len_at(group_index);
        let start = start.min(node_len);
        group_segment_subrange_at::<NodeSegment>(
            &self.groups,
            self.nodes.len(),
            group_index,
            start,
            node_len,
        )
    }

    fn insert_group_node(&mut self, group_index: usize, node_index: usize, node: NodeRecord) {
        self.record_segment_range_update_from(group_index);
        insert_group_segment_item::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            group_index,
            node_index,
            node,
        );
    }

    fn remove_node_range(&mut self, node_range: GroupNodeRange) -> Vec<NodeRecord> {
        if !node_range.is_empty() {
            self.record_segment_range_update_from(node_range.group_index());
        }
        remove_group_segment_range::<NodeSegment, _>(&mut self.groups, &mut self.nodes, node_range)
    }

    fn extract_node_segment_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<NodeRecord> {
        if removed_groups.iter().any(|group| group.node_len > 0) {
            let active_span = self.groups.len().saturating_sub(removed_group_index);
            self.record_segment_range_update_span(active_span + removed_groups.len());
        }
        extract_subtree_segment::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            removed_group_index,
            removed_groups,
        )
    }

    fn restore_node_segment_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        nodes: Vec<NodeRecord>,
    ) {
        if !nodes.is_empty() {
            let active_span = self.groups.len().saturating_sub(insert_group_index);
            self.record_segment_range_update_span(active_span + groups.len());
        }
        restore_subtree_segment::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            insert_group_index,
            groups,
            nodes,
        );
    }

    pub(in crate::slot) fn group_node_records_at(&self, group_index: usize) -> &[NodeRecord] {
        let range = self.group_node_range_at(group_index);
        &self.nodes[range.as_range()]
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

    fn record_group_node(
        &mut self,
        group_index: usize,
        node_index: usize,
        owner: AnchorId,
        id: NodeId,
        parent_id: Option<NodeId>,
        generation: u32,
    ) -> NodeSlotUpdate {
        if node_index < self.group_node_len_at(group_index) {
            let existing = *self.group_node_record_at(group_index, node_index);
            *self.group_node_record_at_mut(group_index, node_index) = NodeRecord {
                owner,
                id,
                parent_id,
                generation,
                lifecycle: NodeLifecycle::Active,
            };
            if existing.id == id && existing.generation == generation {
                NodeSlotUpdate::Reused { id, generation }
            } else {
                NodeSlotUpdate::Replaced {
                    old_id: existing.id,
                    old_generation: existing.generation,
                    new_id: id,
                    new_generation: generation,
                }
            }
        } else {
            self.insert_group_node(
                group_index,
                node_index,
                NodeRecord {
                    owner,
                    id,
                    parent_id,
                    generation,
                    lifecycle: NodeLifecycle::Active,
                },
            );
            NodeSlotUpdate::Inserted { id, generation }
        }
    }

    pub(super) fn record_node_at_cursor(
        &mut self,
        owner: AnchorId,
        node_index: usize,
        id: NodeId,
        parent_id: Option<NodeId>,
        generation: u32,
    ) -> NodeSlotUpdate {
        let group_index = self.current_group_index(owner);
        let update =
            self.record_group_node(group_index, node_index, owner, id, parent_id, generation);
        if matches!(update, NodeSlotUpdate::Inserted { .. }) {
            self.adjust_ancestor_node_counts(owner, 1);
        }

        update
    }

    pub(super) fn node_identity_at_cursor(
        &self,
        owner: AnchorId,
        node_index: usize,
    ) -> Option<(NodeId, u32)> {
        let group_index = self.current_group_index(owner);
        if node_index >= self.group_node_len_at(group_index) {
            return None;
        }
        let node = self.group_node_record_at(group_index, node_index);
        Some((node.id, node.generation))
    }

    pub(super) fn remove_group_node_range(
        &mut self,
        node_range: GroupNodeRange,
    ) -> Vec<NodeRecord> {
        if node_range.is_empty() {
            return Vec::new();
        }
        self.remove_node_range(node_range)
    }

    pub(super) fn remove_group_node_tail_at_cursor(
        &mut self,
        owner: AnchorId,
        node_cursor: usize,
    ) -> Vec<NodeRecord> {
        let group_index = self.current_group_index(owner);
        let node_range = self.group_node_tail_range_at(group_index, node_cursor);
        let removed = self.remove_group_node_range(node_range);
        if !removed.is_empty() {
            let removed_len = checked_usize_to_i64(removed.len(), "removed node count");
            self.adjust_ancestor_node_counts(owner, -removed_len);
        }
        removed
    }

    pub(super) fn detach_nodes_for_groups(
        &mut self,
        removed_group_index: usize,
        removed_groups: &mut [GroupRecord],
    ) -> Vec<NodeRecord> {
        self.extract_node_segment_for_groups(removed_group_index, removed_groups)
    }

    pub(super) fn restore_nodes_for_groups(
        &mut self,
        insert_group_index: usize,
        groups: &mut [GroupRecord],
        nodes: Vec<NodeRecord>,
    ) {
        self.restore_node_segment_for_groups(insert_group_index, groups, nodes);
    }
}
