use super::{
    super::{
        ChildCursor, DetachedSubtree, FinishGroupResult, SlotLifecycleCoordinator, SlotTable,
        SlotWriteSession,
    },
    SlotWriteSessionState,
};

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

    fn finish_group_body_internal(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) -> FinishGroupResult {
        let (group_anchor, payload_cursor, node_cursor, was_skipped) = {
            let Some(frame) = state.group_stack.last_mut() else {
                log::error!("slot writer finish_group_body called with an empty group stack");
                return FinishGroupResult::empty();
            };
            if !frame.mark_body_finished() {
                return FinishGroupResult::empty();
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
        #[cfg(any(test, debug_assertions))]
        state.debug_assert_no_pending_payload_location_refreshes("finish_group_body");

        let mut direct_nodes = Vec::new();
        let removed = self.remove_group_node_tail_at_cursor(group_anchor, node_cursor);
        if !removed.is_empty() {
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let mut detached_children = self.detach_unvisited_children_internal(state);
        let finishing_transparent_site = self
            .active_group_index(group_anchor)
            .and_then(|index| self.groups.get(index))
            .filter(|group| group.transparent)
            .map(|_| self.branch_path_key(group_anchor));
        if let Some(branch_site) = finishing_transparent_site {
            // A branch bracket's departed keyed children wait out the pass
            // under the nearest real group: a shifted bracket of the same
            // branch site may claim them by key, and whatever stays unclaimed
            // flows into that group's own finish — the exact place this
            // content was disposed or retained before brackets existed.
            let owner = self.nearest_non_transparent_ancestor(group_anchor);
            let mut kept = Vec::new();
            for subtree in detached_children.drain(..) {
                match subtree.root_key_checked() {
                    Some(key) if key.explicit_key.is_some() => {
                        state.park_orphaned_keyed(owner, branch_site, key, subtree);
                    }
                    _ => kept.push(subtree),
                }
            }
            detached_children = kept;
        } else if state.has_orphaned_keyed() {
            detached_children.extend(state.drain_orphaned_keyed_for_owner(group_anchor));
        }
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
