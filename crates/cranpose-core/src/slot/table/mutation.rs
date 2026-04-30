use super::super::{
    checked_u32_delta, checked_usize_to_u32, ActiveSubtreeRoot, CheckedU32Delta, ChildCursor,
    GroupKey, GroupRecord,
};
use super::SlotTable;
use crate::AnchorId;

#[cfg(any(test, debug_assertions))]
pub(in crate::slot) struct SlotMutationGuard<'a> {
    table: &'a SlotTable,
    operation: &'static str,
}

#[cfg(any(test, debug_assertions))]
impl<'a> SlotMutationGuard<'a> {
    pub(in crate::slot) fn new(table: &'a SlotTable, operation: &'static str) -> Self {
        Self { table, operation }
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for SlotMutationGuard<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.table.debug_assert_valid_after(self.operation);
        }
    }
}

impl SlotTable {
    fn allocate_group_anchor(&mut self) -> AnchorId {
        self.anchors.allocate()
    }

    pub(super) fn allocate_group_generation(&mut self) -> u32 {
        let generation = self.next_group_generation;
        self.next_group_generation = self
            .next_group_generation
            .checked_add(1)
            .expect("group generation counter overflow");
        generation
    }

    pub(in crate::slot) fn record_segment_range_update_from(&mut self, start: usize) {
        let group_span = self.groups.len().saturating_sub(start);
        self.mutation_debug_stats
            .record_segment_range_update(group_span);
    }

    pub(in crate::slot) fn record_segment_range_update_span(&mut self, group_span: usize) {
        self.mutation_debug_stats
            .record_segment_range_update(group_span);
    }

    pub(in crate::slot) fn adjust_ancestor_group_spans(
        &mut self,
        parent_anchor: AnchorId,
        subtree_delta: i64,
        node_delta: i64,
    ) {
        let subtree_delta = CheckedU32Delta::from_i64(subtree_delta, "active group subtree span");
        let node_delta = CheckedU32Delta::from_i64(node_delta, "active group subtree node count");
        let mut current = parent_anchor;
        while current.is_valid() {
            let group_index = self.current_group_index(current);
            let group = &mut self.groups[group_index];
            group.subtree_len = checked_u32_delta(
                group.subtree_len,
                subtree_delta,
                1,
                "active group subtree span",
            );
            group.subtree_node_count = checked_u32_delta(
                group.subtree_node_count,
                node_delta,
                0,
                "active group subtree node count",
            );
            current = group.parent_anchor;
        }
    }

    pub(in crate::slot) fn adjust_ancestor_node_counts(&mut self, owner: AnchorId, delta: i64) {
        let delta = CheckedU32Delta::from_i64(delta, "active group subtree node count");
        let mut current = Some(owner);
        while let Some(anchor) = current {
            let group_index = self.current_group_index(anchor);
            let group = &mut self.groups[group_index];
            group.subtree_node_count = checked_u32_delta(
                group.subtree_node_count,
                delta,
                0,
                "active group subtree node count",
            );
            current = group
                .parent_anchor
                .is_valid()
                .then_some(group.parent_anchor);
        }
    }

    pub(in crate::slot) fn insert_new_group(
        &mut self,
        cursor: ChildCursor,
        key: GroupKey,
    ) -> AnchorId {
        // Group insertion recipe:
        // allocate identity, insert the group record, refresh active indexes from
        // the insertion point, then update ancestor spans.
        self.assert_child_cursor_boundary(cursor);
        let insert_index = cursor.index();
        let parent_anchor = cursor.parent();
        let depth = if parent_anchor.is_valid() {
            self.current_group(parent_anchor)
                .depth
                .checked_add(1)
                .expect("group depth overflow")
        } else {
            0
        };
        let anchor = self.allocate_group_anchor();
        let generation = self.allocate_group_generation();
        let payload_start = if insert_index < self.groups.len() {
            self.groups[insert_index].payload_start
        } else {
            checked_usize_to_u32(self.payloads.len(), "group payload start")
        };
        let node_start = if insert_index < self.groups.len() {
            self.groups[insert_index].node_start
        } else {
            checked_usize_to_u32(self.nodes.len(), "group node start")
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

    pub(in crate::slot) fn move_subtree(&mut self, root: ActiveSubtreeRoot, cursor: ChildCursor) {
        self.assert_child_cursor_boundary(cursor);
        let anchor = root.anchor();
        let from_index = self.current_group_index(anchor);
        let moving_groups = self.group_subtree_range_at_index(from_index);
        let insert_index = cursor.index();
        if from_index == insert_index {
            return;
        }

        let moving_root = self.group_sibling_record_at_index(from_index);
        assert_eq!(
            moving_root.parent_anchor,
            cursor.parent(),
            "moved subtree root must be a direct child of the cursor parent"
        );
        assert!(
            insert_index < from_index,
            "moved subtree root must be a later direct sibling of the child cursor"
        );

        // Move recipe:
        // extract group, payload, and node segments from the later sibling;
        // restore payload and node segments at the earlier cursor; splice groups;
        // record movement stats; refresh active indexes over the affected suffix.
        let mut moved = self
            .groups
            .drain(moving_groups.as_range())
            .collect::<Vec<_>>();
        let moved_payloads = self.move_payloads_for_groups(from_index, &mut moved);
        let moved_nodes = self.detach_nodes_for_groups(from_index, &mut moved);
        let moved_group_count = moved.len();
        let moved_payload_count = moved_payloads.len();
        let moved_node_count = moved_nodes.len();
        let adjusted_index = if insert_index > moving_groups.root_index() {
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
        #[cfg(any(test, debug_assertions))]
        let _guard = SlotMutationGuard::new(self, "move_subtree");
    }
}
