use super::super::{
    DetachedSubtree, FinishGroupResult, SlotLifecycleCoordinator, SlotTable, SlotWriteSession,
};
use super::SlotWriteSessionState;

impl SlotTable {
    fn detach_unvisited_children_internal(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) -> Vec<DetachedSubtree> {
        let (parent_anchor, next_child_index) = {
            let frame = state
                .group_stack
                .last()
                .expect("detach_unvisited_children requires an active group");
            (frame.group_anchor, frame.next_child_index)
        };
        let mut detached_children = Vec::new();
        while let Some(anchor) = self.direct_child_anchor_at(parent_anchor, next_child_index) {
            detached_children.push(self.detach_subtree(anchor));
        }
        state.note_detached_subtrees(&detached_children);
        detached_children
    }

    fn finish_group_body_internal(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) -> FinishGroupResult {
        let (group_anchor, payload_cursor, node_cursor, was_skipped) = {
            let frame = state
                .group_stack
                .last_mut()
                .expect("finish_group_body requires an active group");
            if !frame.mark_body_finished() {
                return FinishGroupResult {
                    detached_children: Vec::new(),
                    direct_nodes: Vec::new(),
                    root_nodes: Vec::new(),
                    was_skipped: false,
                };
            }

            (
                frame.group_anchor,
                frame.payload_cursor,
                frame.node_cursor,
                frame.was_skipped,
            )
        };

        let group_index = self.current_group_index(group_anchor);
        let payload_len = self.group_payload_len_at(group_index);
        if payload_cursor < payload_len {
            let payload_range =
                self.group_payload_subrange_at(group_index, payload_cursor, payload_len);
            let removed = self.remove_payload_range(group_anchor, payload_range);
            let removed_payload_count = removed.len();
            for payload in removed {
                lifecycle.queue_drop(payload.into_deferred_drop());
            }
            state.note_removed_payloads(removed_payload_count);
        }

        let mut direct_nodes = Vec::new();
        let node_range = self.group_node_tail_range_at(group_index, node_cursor);
        let removed = self.remove_group_node_range(node_range);
        if !removed.is_empty() {
            self.adjust_ancestor_node_counts(group_anchor, -(removed.len() as i32));
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let detached_children = self.detach_unvisited_children_internal(state);
        let root_nodes = if was_skipped {
            self.collect_subtree_root_node_ids(group_anchor)
        } else {
            Vec::new()
        };
        state.note_removed_nodes(direct_nodes.len());
        FinishGroupResult {
            detached_children,
            direct_nodes,
            root_nodes,
            was_skipped,
        }
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn finish_group_body(&mut self) -> FinishGroupResult {
        self.table
            .finish_group_body_internal(self.lifecycle, self.state)
    }
}
