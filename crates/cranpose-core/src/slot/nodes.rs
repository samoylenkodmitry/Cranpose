use super::{
    GroupNodeRange, GroupRecord, NodeRange, NodeRecord, NodeSlotUpdate, SlotTable,
    checked_usize_to_i64, checked_usize_to_u32,
    segments::{
        GroupSegment, NodeSegment, add_group_segment_len, extract_subtree_segment,
        group_segment_len, group_segment_len_checked, group_segment_range_checked,
        group_segment_start, group_segment_subrange_at, insert_group_segment_item,
        move_subtree_segment_to_earlier_group, remove_group_segment_range, restore_subtree_segment,
        shift_group_segment_starts_from,
    },
};
use crate::{AnchorId, NodeId};

// A record past its group's last one waits to join the group with the ones
// after it only past this many records of other groups. Each later record
// in place would move them all again, where the join moves them once; with
// fewer the join costs more than a move in place, and in a group that puts
// one such record, as a tile emits a node after its label, it costs more
// than the move it saves.
const LATE_PAST_RECORDS: usize = 64;

impl SlotTable {
    pub(in crate::slot) fn group_node_start_at(&self, group_index: usize) -> usize {
        group_segment_start::<NodeSegment>(&self.groups, group_index)
    }

    pub(in crate::slot) fn group_node_len_at(&self, group_index: usize) -> usize {
        group_segment_len::<NodeSegment>(&self.groups, group_index)
    }

    fn group_node_range_checked_at(&self, group_index: usize) -> Option<NodeRange> {
        group_segment_range_checked::<NodeSegment>(&self.groups, self.nodes.len(), group_index)
    }

    fn group_node_len_checked_at(&self, group_index: usize, operation: &'static str) -> usize {
        group_segment_len_checked::<NodeSegment>(
            &self.groups,
            self.nodes.len(),
            group_index,
            operation,
        )
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
        self.note_run_insert();
        self.record_segment_range_update_from(group_index);
        insert_group_segment_item::<NodeSegment, _>(
            &mut self.groups,
            &mut self.nodes,
            group_index,
            node_index,
            node,
        );
    }

    // Moves the last `count` records of `late`, the group's own, to the end
    // of the segment of the group at `group_index`, moving the records after
    // it once for all of them.
    pub(in crate::slot) fn append_group_nodes(
        &mut self,
        group_index: usize,
        late: &mut Vec<NodeRecord>,
        count: usize,
    ) {
        let from = late.len() - count;
        let Some(range) = self.group_node_range_checked_at(group_index) else {
            log::error!(
                "slot table dropped {count} node records for corrupt node segment at group index {group_index}"
            );
            late.truncate(from);
            return;
        };
        let end = range.as_range().end;
        self.note_run_insert();
        self.record_segment_range_update_from(group_index);
        self.nodes.extend_from_slice(&late[from..]);
        late.truncate(from);
        self.nodes[end..].rotate_right(count);
        let owner = self.groups[group_index].anchor;
        self.count_appended_group_nodes(
            group_index,
            owner,
            checked_usize_to_u32(count, NodeSegment::NAME),
        );
    }

    // Counts `count` records just put at the end of the segment of the group
    // at `group_index`, whose anchor is `owner`, in that segment, in the
    // starts of the segments after it and in the spans of the group and its
    // ancestors.
    fn count_appended_group_nodes(&mut self, group_index: usize, owner: AnchorId, count: u32) {
        let count = i64::from(count);
        add_group_segment_len::<NodeSegment>(&mut self.groups, group_index, count);
        shift_group_segment_starts_from::<NodeSegment>(&mut self.groups, group_index + 1, count);
        self.adjust_ancestor_group_spans(owner, 0, count);
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
        self.nodes.capacity() * size_of::<NodeRecord>()
    }

    pub(super) fn node_debug_capacity(&self) -> usize {
        self.nodes.capacity()
    }

    #[cfg(test)]
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
        self.record_node_at(
            group_index,
            node_index,
            NodeRecord::active(owner, id, parent_id, generation, source, 0),
            &mut Vec::new(),
        )
    }

    // Records `record` at `node_index` of its owner's group, which sits at
    // `group_index`. A record past the group's last one, while many records
    // of other groups follow the group's segment, waits in `late` for the
    // group's body to finish, and so does each record after it: the node
    // cursor then stands past the group's records.
    pub(super) fn record_node_at(
        &mut self,
        group_index: usize,
        node_index: usize,
        record: NodeRecord,
        late: &mut Vec<NodeRecord>,
    ) -> NodeSlotUpdate {
        let node_len = self.group_node_len_checked_at(group_index, "node cursor");
        let (id, generation) = (record.id, record.generation);
        if node_index < node_len {
            let existing = *self.group_node_record_at(group_index, node_index);
            if existing.id == id {
                *self.group_node_record_at_mut(group_index, node_index) = record;
                return if existing.generation == generation {
                    NodeSlotUpdate::Reused { id, generation }
                } else {
                    NodeSlotUpdate::Replaced {
                        old_id: existing.id,
                        old_generation: existing.generation,
                        new_id: id,
                        new_generation: generation,
                    }
                };
            }
        } else {
            let end = self.group_node_start_at(group_index) + node_len;
            match self.nodes.len().checked_sub(end) {
                Some(past) if node_index == node_len && past <= LATE_PAST_RECORDS => {
                    let owner = record.owner;
                    self.note_run_insert();
                    self.record_segment_range_update_from(group_index);
                    self.nodes.insert(end, record);
                    self.count_appended_group_nodes(group_index, owner, 1);
                }
                _ => late.push(record),
            }
            return NodeSlotUpdate::Inserted { id, generation };
        }
        let owner = record.owner;
        self.insert_group_node(group_index, node_index, record);
        self.adjust_ancestor_group_spans(owner, 0, 1);
        NodeSlotUpdate::Inserted { id, generation }
    }

    pub(super) fn find_node_record_by_source_at(
        &self,
        group_index: usize,
        from_index: usize,
        source: crate::Key,
    ) -> Option<(usize, NodeId, u32)> {
        let records = self.group_node_records_at(group_index);
        (from_index..records.len()).find_map(|index| {
            let record = &records[index];
            (record.source == source).then_some((index, record.id, record.generation))
        })
    }

    pub(super) fn rotate_node_record_to_cursor_at(
        &mut self,
        group_index: usize,
        found_index: usize,
        cursor_index: usize,
    ) {
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
                "slot table ignored node removal: requested range {requested_range:?} is outside current group node range {current_range:?}"
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
        let node_range = self.group_node_tail_range_at(group_index, node_cursor);
        let removed = self.remove_group_node_range(node_range);
        if !removed.is_empty() {
            let removed_len = checked_usize_to_i64(removed.len(), "removed node count");
            self.adjust_ancestor_group_spans(owner, 0, -removed_len);
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
