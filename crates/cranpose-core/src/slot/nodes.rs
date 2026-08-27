use std::mem;

use super::{
    GroupNodeRange, GroupRecord, NodeLifecycle, NodeRange, NodeRecord, NodeSlotUpdate, SlotTable,
    checked_usize_to_i64,
    segments::{
        NodeSegment, extract_subtree_segment, group_segment_len, group_segment_range_checked,
        group_segment_start, group_segment_subrange_at, insert_group_segment_item,
        move_subtree_segment_to_earlier_group, remove_group_segment_range,
        repair_group_segment_start_and_len_to_storage, restore_subtree_segment,
    },
};
use crate::{AnchorId, NodeId};

impl SlotTable {
    fn group_node_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<NodeSegment>(&self.groups, group_index)
    }

    pub(in crate::slot) fn group_node_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<NodeSegment>(&self.groups, group_index)
    }

    fn group_node_range_checked_at(&self, group_index: usize) -> Option<NodeRange> {
        group_segment_range_checked::<NodeSegment>(&self.groups, self.nodes.len(), group_index)
    }

    pub(in crate::slot) fn repair_group_node_len_to_storage(
        &mut self,
        group_index: usize,
        operation: &'static str,
    ) -> usize {
        let repair = repair_group_segment_start_and_len_to_storage::<NodeSegment>(
            &mut self.groups,
            self.nodes.len(),
            group_index,
            operation,
        );
        if repair.repaired {
            self.record_segment_range_update_from(group_index);
        }
        repair.len
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
        // Node insertion/removal uses the same segment recipe as payloads, with
        // ancestor node counts adjusted by the public cursor-level operations.
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
        // The caller owns lifecycle side effects for removed node records after
        // this function restores coherent node segment ranges.
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
        let Some(range) = self.group_node_range_checked_at(group_index) else {
            log::error!(
                "slot table ignored node record read for corrupt node segment at group index {group_index}"
            );
            return &[];
        };
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
        record: NodeRecord,
    ) -> NodeSlotUpdate {
        let node_len = self.repair_group_node_len_to_storage(group_index, "node cursor");
        let node_index = if node_index > node_len {
            log::error!(
                "slot table clamped node cursor {node_index} to repaired node length {node_len} for owner {:?}",
                record.owner
            );
            node_len
        } else {
            node_index
        };
        let id = record.id;
        let generation = record.generation;
        if node_index < node_len {
            let existing = *self.group_node_record_at(group_index, node_index);
            if existing.id == id {
                *self.group_node_record_at_mut(group_index, node_index) = record;
                if existing.generation == generation {
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
                self.insert_group_node(group_index, node_index, record);
                NodeSlotUpdate::Inserted { id, generation }
            }
        } else {
            self.insert_group_node(group_index, node_index, record);
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
        source: crate::Key,
    ) -> NodeSlotUpdate {
        let Some(group_index) = self.active_group_index(owner) else {
            log::error!(
                "slot table ignored node record for stale owner anchor {owner:?}; node id={id}"
            );
            return NodeSlotUpdate::Inserted { id, generation };
        };
        let update = self.record_group_node(
            group_index,
            node_index,
            NodeRecord {
                owner,
                id,
                parent_id,
                generation,
                source,
                lifecycle: NodeLifecycle::Active,
            },
        );
        if matches!(update, NodeSlotUpdate::Inserted { .. }) {
            self.adjust_ancestor_node_counts(owner, 1);
        }

        update
    }

    pub(super) fn find_node_record_by_source(
        &self,
        owner: AnchorId,
        from_index: usize,
        source: crate::Key,
    ) -> Option<(usize, NodeId, u32)> {
        let group_index = self.active_group_index(owner)?;
        let records = self.group_node_records_at(group_index);
        (from_index..records.len()).find_map(|index| {
            let record = &records[index];
            (record.source == source).then_some((index, record.id, record.generation))
        })
    }

    pub(super) fn rotate_node_record_to_cursor(
        &mut self,
        owner: AnchorId,
        found_index: usize,
        cursor_index: usize,
    ) {
        let Some(group_index) = self.active_group_index(owner) else {
            return;
        };
        let start = self.group_node_start_at(group_index);
        self.nodes[start + cursor_index..=start + found_index].rotate_right(1);
    }

    #[cfg(test)]
    pub(super) fn node_identity_at_cursor(
        &self,
        owner: AnchorId,
        node_index: usize,
    ) -> Option<(NodeId, u32, crate::Key)> {
        let group_index = self.active_group_index(owner)?;
        if node_index >= self.group_node_len_at(group_index) {
            return None;
        }
        let node = self.group_node_records_at(group_index).get(node_index)?;
        Some((node.id, node.generation, node.source))
    }

    pub(super) fn remove_group_node_range(
        &mut self,
        node_range: GroupNodeRange,
    ) -> Vec<NodeRecord> {
        if node_range.is_empty() {
            return Vec::new();
        }
        let group_index = node_range.group_index();
        let Some(current_range) = self.group_node_range_checked_at(group_index) else {
            log::error!(
                "slot table ignored node removal for corrupt node segment at group index {group_index}"
            );
            return Vec::new();
        };
        let requested_range = node_range.as_range();
        let current_range = current_range.as_range();
        if requested_range.start < current_range.start || requested_range.end > current_range.end {
            log::error!(
                "slot table ignored node removal: requested range {:?} is outside current group node range {:?}",
                requested_range,
                current_range
            );
            return Vec::new();
        }
        self.remove_node_range(node_range)
    }

    pub(super) fn remove_group_node_tail_at_cursor(
        &mut self,
        owner: AnchorId,
        node_cursor: usize,
    ) -> Vec<NodeRecord> {
        let Some(group_index) = self.active_group_index(owner) else {
            log::error!("slot table ignored node-tail removal for stale owner anchor {owner:?}");
            return Vec::new();
        };
        self.repair_group_node_len_to_storage(group_index, "node tail cleanup");
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

    pub(super) fn move_nodes_to_earlier_group(
        &mut self,
        insert_group_index: usize,
        moving_group_index: usize,
        moving_group_len: usize,
    ) -> usize {
        let moved_node_count = move_subtree_segment_to_earlier_group::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            insert_group_index,
            moving_group_index,
            moving_group_len,
        );
        if moved_node_count > 0 {
            self.record_segment_range_update_span(
                moving_group_index + moving_group_len - insert_group_index,
            );
        }
        moved_node_count
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
