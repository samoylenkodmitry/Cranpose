use super::super::{
    ChildCursor, DetachedSubtree, FinishGroupResult, SlotLifecycleCoordinator, SlotTable,
    SlotWriteSession,
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
        let cursor = ChildCursor::new(parent_anchor, next_child_index);
        let detached_children = self.detach_subtrees_at_cursor(cursor);
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

        {
            let removed = self.remove_payload_tail_at_cursor(group_anchor, payload_cursor);
            if !removed.is_empty() {
                let removed_payload_count = removed.len();
                for payload in removed {
                    lifecycle.queue_drop(payload.into_deferred_drop());
                }
                state.note_removed_payloads(removed_payload_count);
            }
        }
        self.flush_payload_location_refreshes(state);

        let mut direct_nodes = Vec::new();
        let removed = self.remove_group_node_tail_at_cursor(group_anchor, node_cursor);
        if !removed.is_empty() {
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let detached_children = self.detach_unvisited_children_internal(state);
        let root_nodes = if was_skipped {
            self.collect_subtree_root_node_ids(group_anchor)
        } else {
            Vec::new()
        };
        state.note_removed_nodes(direct_nodes.len());
        let result = FinishGroupResult {
            detached_children,
            direct_nodes,
            root_nodes,
            was_skipped,
        };

        #[cfg(any(test, debug_assertions))]
        if state.group_stack.len() == 1 {
            self.debug_assert_valid_after("finish_group_body");
        }

        result
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn finish_group_body(&mut self) -> FinishGroupResult {
        self.table
            .finish_group_body_internal(self.lifecycle, self.state)
    }
}
