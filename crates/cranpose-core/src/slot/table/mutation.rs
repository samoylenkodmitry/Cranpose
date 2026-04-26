use super::super::{GroupRange, GroupRecord};
use super::SlotTable;
use crate::{slot_storage::GroupKey, AnchorId};

impl SlotTable {
    fn allocate_group_anchor(&mut self) -> AnchorId {
        self.anchors.allocate()
    }

    fn allocate_group_generation(&mut self) -> u32 {
        let generation = self.next_group_generation;
        self.next_group_generation = self
            .next_group_generation
            .checked_add(1)
            .expect("group generation counter overflow");
        generation
    }

    pub(in crate::slot) fn adjust_ancestor_group_spans(
        &mut self,
        parent_anchor: AnchorId,
        subtree_delta: i32,
        node_delta: i32,
    ) {
        let mut current = parent_anchor;
        while current.is_valid() {
            let group_index = self.current_group_index(current);
            let group = &mut self.groups[group_index];
            let subtree_len = group.subtree_len as i32 + subtree_delta;
            let subtree_nodes = group.subtree_node_count as i32 + node_delta;
            debug_assert!(
                subtree_len >= 1,
                "active groups must keep a positive subtree span"
            );
            debug_assert!(
                subtree_nodes >= 0,
                "subtree node counts cannot become negative"
            );
            group.subtree_len = subtree_len as u32;
            group.subtree_node_count = subtree_nodes as u32;
            current = group.parent_anchor;
        }
    }

    pub(in crate::slot) fn adjust_ancestor_node_counts(&mut self, owner: AnchorId, delta: i32) {
        let mut current = Some(owner);
        while let Some(anchor) = current {
            let group_index = self.current_group_index(anchor);
            let group = &mut self.groups[group_index];
            let updated = group.subtree_node_count as i32 + delta;
            debug_assert!(updated >= 0, "subtree node counts cannot become negative");
            group.subtree_node_count = updated as u32;
            current = group
                .parent_anchor
                .is_valid()
                .then_some(group.parent_anchor);
        }
    }

    pub(in crate::slot) fn insert_new_group(
        &mut self,
        insert_index: usize,
        parent_anchor: AnchorId,
        key: GroupKey,
    ) -> AnchorId {
        let depth = if parent_anchor.is_valid() {
            self.current_group(parent_anchor).depth + 1
        } else {
            0
        };
        let anchor = self.allocate_group_anchor();
        let generation = self.allocate_group_generation();
        let payload_start = if insert_index < self.groups.len() {
            self.groups[insert_index].payload_start
        } else {
            self.payloads.len() as u32
        };
        let node_start = if insert_index < self.groups.len() {
            self.groups[insert_index].node_start
        } else {
            self.nodes.len() as u32
        };
        self.groups.insert(
            insert_index,
            GroupRecord {
                key,
                parent_anchor,
                depth,
                subtree_len: 1,
                payload_start,
                payload_len: 0,
                node_start,
                node_len: 0,
                subtree_node_count: 0,
                generation,
                anchor,
                scope_id: None,
            },
        );
        self.refresh_group_indexes_from(insert_index);
        self.adjust_ancestor_group_spans(parent_anchor, 1, 0);
        anchor
    }

    pub(in crate::slot) fn move_subtree(&mut self, anchor: AnchorId, insert_index: usize) {
        let from_index = self.current_group_index(anchor);
        if from_index == insert_index {
            return;
        }
        let subtree_len = self.groups[from_index].subtree_len as usize;
        let moving_groups = GroupRange::from_start_len(from_index, subtree_len);
        let mut moved = self
            .groups
            .drain(moving_groups.as_range())
            .collect::<Vec<_>>();
        let moved_payloads = self.move_payloads_for_groups(from_index, &mut moved);
        let moved_nodes = self.detach_nodes_for_groups(from_index, &mut moved);
        let moved_group_count = moved.len();
        let moved_payload_count = moved_payloads.len();
        let moved_node_count = moved_nodes.len();
        let adjusted_index = if insert_index > from_index {
            insert_index - moving_groups.len()
        } else {
            insert_index
        };
        self.restore_payloads_for_groups(adjusted_index, &mut moved, moved_payloads);
        self.restore_nodes_for_groups(adjusted_index, &mut moved, moved_nodes);
        self.groups.splice(adjusted_index..adjusted_index, moved);
        self.mutation_debug_stats.record_subtree_move(
            moved_group_count,
            moved_payload_count,
            moved_node_count,
        );
        self.refresh_group_indexes_from(from_index.min(adjusted_index));
    }
}
