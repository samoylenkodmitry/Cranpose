use super::{
    super::{
        ChildCursor, DetachedSubtree, FinishGroupResult, RootNodeIds, SlotLifecycleCoordinator,
        SlotTable, SlotWriteSession,
    },
    SlotWriteSessionState,
};
use crate::AnchorId;

struct GroupLeftovers {
    payloads: bool,
    nodes: bool,
    children: bool,
}

impl SlotTable {
    fn detach_unvisited_children_internal(
        &mut self,
        state: &mut SlotWriteSessionState,
    ) -> Vec<DetachedSubtree> {
        let (parent_anchor, next_child_index) = {
            let Some(frame) = state.group_stack.last() else {
                log::error!(
                    "slot writer detach_unvisited_children called with an empty group stack"
                );
                return Vec::new();
            };
            (frame.group_anchor, frame.next_child_index)
        };
        let cursor = ChildCursor::new(parent_anchor, next_child_index);
        let detached_children = self.detach_subtrees_at_cursor(cursor);
        state.note_detached_subtrees(&detached_children);
        detached_children
    }

    fn open_group_leftovers(
        &self,
        group_anchor: AnchorId,
        group_index: usize,
        payload_cursor: usize,
        node_cursor: usize,
        next_child_index: usize,
    ) -> GroupLeftovers {
        let Some(group) = self
            .open_group_index(group_anchor, group_index)
            .and_then(|index| self.groups.get(index).map(|group| (index, group)))
        else {
            return GroupLeftovers {
                payloads: true,
                nodes: true,
                children: true,
            };
        };
        let (index, group) = group;
        let children_end = index.saturating_add(group.subtree_len as usize);
        GroupLeftovers {
            payloads: payload_cursor < group.payload_len as usize,
            nodes: node_cursor < group.node_len as usize,
            children: index < next_child_index && next_child_index < children_end,
        }
    }

    fn finish_group_body_internal(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) -> FinishGroupResult {
        let (group_anchor, group_index, payload_cursor, node_cursor, next_child_index, was_skipped) = {
            let Some(frame) = state.group_stack.last_mut() else {
                log::error!("slot writer finish_group_body called with an empty group stack");
                return FinishGroupResult::empty();
            };
            if !frame.mark_body_finished() {
                return FinishGroupResult::empty();
            }
            if frame.untouched_since_skip() {
                return FinishGroupResult {
                    root_nodes: self
                        .open_frame_root_node_ids(frame.group_anchor, frame.group_index),
                    was_skipped: true,
                    ..FinishGroupResult::empty()
                };
            }

            (
                frame.group_anchor,
                frame.group_index,
                frame.payload_cursor,
                frame.node_cursor,
                frame.next_child_index,
                frame.was_skipped(),
            )
        };
        let leftovers = self.open_group_leftovers(
            group_anchor,
            group_index,
            payload_cursor,
            node_cursor,
            next_child_index,
        );

        if leftovers.payloads {
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
        #[cfg(any(test, debug_assertions))]
        state.debug_assert_no_pending_payload_location_refreshes("finish_group_body");

        let mut direct_nodes = Vec::new();
        if leftovers.nodes {
            let removed = self.remove_group_node_tail_at_cursor(group_anchor, node_cursor);
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let detached_children = if leftovers.children {
            self.detach_unvisited_children_internal(state)
        } else {
            Vec::new()
        };
        let root_nodes = if was_skipped {
            self.open_frame_root_node_ids(group_anchor, group_index)
        } else {
            RootNodeIds::new()
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

    fn open_frame_root_node_ids(&self, group_anchor: AnchorId, group_index: usize) -> RootNodeIds {
        let Some(group_index) = self.open_group_index(group_anchor, group_index) else {
            log::error!(
                "slot writer ignored root-node collection for stale group frame anchor {group_anchor:?}"
            );
            return RootNodeIds::new();
        };
        self.subtree_root_node_ids_at(group_index)
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn finish_group_body(&mut self) -> FinishGroupResult {
        self.table
            .finish_group_body_internal(self.lifecycle, self.state)
    }
}
