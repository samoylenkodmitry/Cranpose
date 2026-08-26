use super::{
    super::{
        ActiveSubtreeRoot, CheckedU32Delta, ChildCursor, GroupKey, GroupRecord, checked_u32_delta,
        checked_usize_to_u32, try_checked_u32_delta,
    },
    SlotTable,
};
use crate::AnchorId;

impl SlotTable {
    fn allocate_group_anchor(&mut self) -> Option<AnchorId> {
        self.anchors.try_allocate()
    }

    pub(super) fn allocate_group_generation(&mut self) -> u32 {
        let generation = self.next_group_generation;
        self.next_group_generation = match self.next_group_generation.checked_add(1) {
            Some(next_generation) => next_generation,
            None => {
                log::error!(
                    "slot table wrapped the group generation counter after reaching u32::MAX"
                );
                1
            }
        };
        generation
    }

    pub(in crate::slot) fn record_segment_range_update_from(&mut self, start: usize) {
        let group_span = self.groups.len().saturating_sub(start);
        self.diagnostics.record_segment_range_update(group_span);
    }

    pub(in crate::slot) fn record_segment_range_update_span(&mut self, group_span: usize) {
        self.diagnostics.record_segment_range_update(group_span);
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
            let Some(group_index) = self.active_group_index(current) else {
                log::error!(
                    "slot table stopped ancestor span adjustment at stale parent anchor {current:?}"
                );
                return;
            };
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
            let Some(group_index) = self.active_group_index(anchor) else {
                log::error!(
                    "slot table stopped ancestor node-count adjustment at stale parent anchor {anchor:?}"
                );
                return;
            };
            let declared_node_count = self.groups[group_index].subtree_node_count;
            if let Some(updated_node_count) = try_checked_u32_delta(declared_node_count, delta, 0) {
                self.groups[group_index].subtree_node_count = updated_node_count;
            } else if self
                .repair_group_subtree_node_count_from_storage(
                    group_index,
                    "ancestor node-count adjustment",
                )
                .is_none()
            {
                log::error!(
                    "slot table stopped ancestor node-count adjustment at unrecoverable group index {group_index} for anchor {anchor:?}"
                );
                return;
            }

            let parent_anchor = self.groups[group_index].parent_anchor;
            current = parent_anchor.is_valid().then_some(parent_anchor);
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
        if !self.repair_child_cursor_parent_subtree(cursor, "group insertion") {
            log::error!(
                "slot table rejected group insertion for unrecoverable child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return AnchorId::INVALID;
        }
        if !self.child_cursor_boundary_is_valid(cursor) {
            log::error!(
                "slot table rejected group insertion for invalid child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return AnchorId::INVALID;
        }
        let insert_index = cursor.index();
        let parent_anchor = cursor.parent();
        let depth = if parent_anchor.is_valid() {
            let Some(parent_index) = self.active_group_index(parent_anchor) else {
                log::error!(
                    "slot table rejected group insertion for stale parent anchor {parent_anchor:?}"
                );
                return AnchorId::INVALID;
            };
            let Some(depth) = self.groups[parent_index].depth.checked_add(1) else {
                log::error!(
                    "slot table rejected group insertion because parent depth overflowed for {parent_anchor:?}"
                );
                return AnchorId::INVALID;
            };
            depth
        } else {
            0
        };
        let Some(anchor) = self.allocate_group_anchor() else {
            log::error!(
                "slot table rejected group insertion because group anchor ids are exhausted"
            );
            return AnchorId::INVALID;
        };
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

    /// Moves a later direct child subtree to an earlier cursor under the same parent.
    ///
    /// This is the writer's keyed sibling reorder primitive. It is not a general
    /// tree move: `root` must identify a direct child of `cursor.parent()`, and
    /// `cursor.index()` must be before the root's current index. Passing the
    /// root's current cursor is a no-op.
    pub(in crate::slot) fn move_later_sibling_subtree_to_cursor(
        &mut self,
        root: ActiveSubtreeRoot,
        cursor: ChildCursor,
    ) {
        if !self.repair_child_cursor_parent_subtree(cursor, "keyed sibling move cursor") {
            log::error!(
                "slot table ignored keyed sibling move for unrecoverable child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return;
        }
        if !self.child_cursor_boundary_is_valid(cursor) {
            log::error!(
                "slot table ignored keyed sibling move for invalid child cursor parent={:?} index={}",
                cursor.parent(),
                cursor.index()
            );
            return;
        }
        let anchor = root.anchor();
        let Some(from_index) = self.active_group_index(anchor) else {
            log::error!("slot table ignored keyed sibling move for stale root anchor {anchor:?}");
            return;
        };
        let Some(moving_groups) =
            self.repair_group_subtree_range_at_index(from_index, "keyed sibling move")
        else {
            log::error!(
                "slot table ignored keyed sibling move for corrupt root subtree at group index {from_index}"
            );
            return;
        };
        let insert_index = cursor.index();
        if from_index == insert_index {
            return;
        }

        let Some(moving_root) = self.group_sibling_record_at_index_checked(from_index) else {
            log::error!(
                "slot table ignored keyed sibling move for missing root group index {from_index}"
            );
            return;
        };
        if moving_root.parent_anchor != cursor.parent() {
            log::error!(
                "slot table ignored keyed sibling move for root anchor {anchor:?} outside cursor parent {:?}",
                cursor.parent()
            );
            return;
        }
        if insert_index >= from_index {
            log::error!(
                "slot table ignored keyed sibling move from index {from_index} to non-earlier cursor {insert_index}"
            );
            return;
        }

        // Move recipe:
        // rotate payload, node, and group segments in place; update the segment
        // starts carried by the affected group records; then refresh active
        // indexes over the range whose preorder indexes actually changed.
        let moved_group_count = moving_groups.len();
        let affected_group_end = moving_groups.as_group_range().end();
        let moved_payload_count =
            self.move_payloads_to_earlier_group(insert_index, from_index, moved_group_count);
        let moved_node_count =
            self.move_nodes_to_earlier_group(insert_index, from_index, moved_group_count);
        self.groups[insert_index..affected_group_end].rotate_right(moved_group_count);
        self.diagnostics.record_subtree_move(
            moved_group_count,
            moved_payload_count,
            moved_node_count,
        );
        self.refresh_group_indexes_in_range(insert_index, affected_group_end);
        #[cfg(any(test, debug_assertions))]
        self.debug_assert_valid_after("move_later_sibling_subtree_to_cursor");
    }
}
