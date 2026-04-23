use super::{
    detach::{dispose_detached_node_now, dispose_detached_subtree_now},
    DeferredDrop, DetachedSubtree, FinishGroupResult, SlotLifecycleCoordinator, SlotTable,
    SlotWriteSession, SlotWriteSessionState,
};
use crate::{
    collections::map::HashMap,
    slot_storage::{
        BeginGroupInput, GroupId, GroupKey, GroupKeySeed, GroupStart, GroupStartKind,
        NodeRecordResult, ValueSlotId,
    },
    AnchorId, Applier, NodeId, Owned, ScopeId,
};

impl SlotTable {
    fn collect_subtree_node_records(&self, group_anchor: AnchorId) -> Vec<super::NodeRecord> {
        let group_index = self.current_group_index(group_anchor);
        let subtree_end = group_index + self.groups[group_index].subtree_len as usize;
        let mut nodes = Vec::with_capacity(self.groups[group_index].subtree_node_count as usize);
        for index in group_index..subtree_end {
            nodes.extend(self.group_node_records_at(index).iter().copied());
        }
        nodes
    }

    fn collect_subtree_node_ids(&self, group_anchor: AnchorId) -> Vec<NodeId> {
        self.collect_subtree_node_records(group_anchor)
            .into_iter()
            .map(|node| node.id)
            .collect()
    }

    fn collect_subtree_root_node_ids(&self, group_anchor: AnchorId) -> Vec<NodeId> {
        let nodes = self.collect_subtree_node_records(group_anchor);
        Self::root_node_ids_from_records(&nodes)
    }

    fn find_later_sibling(
        &self,
        parent_anchor: AnchorId,
        key: GroupKey,
        search_start: usize,
    ) -> Option<usize> {
        const SIBLING_INDEX_THRESHOLD: usize = 16;

        let parent_end = self.direct_child_range_end(parent_anchor);
        if search_start >= parent_end {
            return None;
        }

        let mut probe = search_start;
        let mut direct_children_seen = 0usize;
        while probe < parent_end && direct_children_seen < SIBLING_INDEX_THRESHOLD {
            let group = &self.groups[probe];
            debug_assert_eq!(
                group.parent_anchor, parent_anchor,
                "later sibling scans must stay on direct children"
            );
            direct_children_seen += 1;
            probe += group.subtree_len as usize;
        }

        if direct_children_seen >= SIBLING_INDEX_THRESHOLD {
            let mut sibling_index = HashMap::default();
            let mut index = search_start;
            while index < parent_end {
                let group = &self.groups[index];
                debug_assert_eq!(
                    group.parent_anchor, parent_anchor,
                    "later sibling index must stay on direct children"
                );
                sibling_index.entry(group.key).or_insert(index);
                index += group.subtree_len as usize;
            }
            sibling_index.get(&key).copied()
        } else {
            let mut index = search_start;
            while index < parent_end {
                let group = &self.groups[index];
                debug_assert_eq!(
                    group.parent_anchor, parent_anchor,
                    "later sibling scans must stay on direct children"
                );
                if group.key == key {
                    return Some(index);
                }
                index += group.subtree_len as usize;
            }
            None
        }
    }

    fn open_group_frame(
        &mut self,
        state: &mut SlotWriteSessionState,
        anchor: AnchorId,
        start_kind: GroupStartKind,
    ) -> usize {
        let group_index = self.current_group_index(anchor);
        state.push_group_frame(
            anchor,
            start_kind,
            group_index + 1,
            self.group_payload_len_at(group_index),
            self.group_node_len_at(group_index),
        );
        group_index
    }

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
        let (group_anchor, payload_cursor, node_cursor, start_kind, was_skipped) = {
            let frame = state
                .group_stack
                .last_mut()
                .expect("finish_group_body requires an active group");
            if frame.body_finished {
                return FinishGroupResult {
                    detached_children: Vec::new(),
                    structure_changed: false,
                    direct_nodes: Vec::new(),
                    subtree_nodes: Vec::new(),
                    root_nodes: Vec::new(),
                    was_skipped: false,
                };
            }

            frame.body_finished = true;
            (
                frame.group_anchor,
                frame.payload_cursor,
                frame.node_cursor,
                frame.start_kind,
                frame.was_skipped,
            )
        };

        let payload_len = self.group_payload_len_at(self.current_group_index(group_anchor));
        if payload_cursor < payload_len {
            let removed = self.remove_payload_range(group_anchor, payload_cursor, payload_len);
            for payload in removed {
                lifecycle.queue_drop(payload.into_deferred_drop());
            }
        }

        let mut direct_nodes = Vec::new();
        let group_index = self.current_group_index(group_anchor);
        let removed = self.remove_group_node_range(group_index, node_cursor);
        if !removed.is_empty() {
            self.adjust_ancestor_node_counts(group_anchor, -(removed.len() as i32));
            direct_nodes.extend(removed.into_iter().map(|node| node.id));
        }

        let detached_children = self.detach_unvisited_children_internal(state);
        let (subtree_nodes, root_nodes) = if was_skipped {
            (
                self.collect_subtree_node_ids(group_anchor),
                self.collect_subtree_root_node_ids(group_anchor),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let structure_changed = !matches!(start_kind, GroupStartKind::Reused)
            || !direct_nodes.is_empty()
            || !detached_children.is_empty();
        state.note_removed_nodes(direct_nodes.len());
        FinishGroupResult {
            detached_children,
            structure_changed,
            direct_nodes,
            subtree_nodes,
            root_nodes,
            was_skipped,
        }
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn preview_group_key(&self, seed: GroupKeySeed) -> GroupKey {
        self.state.preview_group_key(seed)
    }

    fn open_started_group(
        &mut self,
        anchor: AnchorId,
        kind: GroupStartKind,
    ) -> GroupStart<GroupId> {
        let group_index = self.table.open_group_frame(self.state, anchor, kind);
        let scope_id = self.table.groups[group_index].scope_id;
        GroupStart {
            group: self.table.group_id_at_index(group_index),
            anchor,
            scope_id,
            kind,
        }
    }

    fn restore_started_group(
        &mut self,
        key: GroupKey,
        restored: DetachedSubtree,
    ) -> GroupStart<GroupId> {
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = *self.state.current_next_child_index();
        let anchor = self
            .table
            .restore_subtree(insert_index, parent_anchor, key, restored);
        self.open_started_group(anchor, GroupStartKind::Restored)
    }

    pub(crate) fn begin_recompose_at_scope(&mut self, scope_id: ScopeId) -> Option<GroupId> {
        let group = self.table.group_for_scope(scope_id)?;
        let anchor = self.table.group_anchor(group);
        self.table
            .open_group_frame(self.state, anchor, GroupStartKind::Reused);
        Some(group)
    }

    pub(crate) fn begin_group(
        &mut self,
        input: BeginGroupInput<DetachedSubtree>,
    ) -> GroupStart<GroupId> {
        let BeginGroupInput { key, restored } = input;
        self.state.consume_group_key(key);
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = *self.state.current_next_child_index();

        if let Some(restored) = restored {
            return self.restore_started_group(key, restored);
        }

        let (anchor, kind) = if let Some(expected_anchor) = self
            .table
            .direct_child_anchor_at(parent_anchor, insert_index)
        {
            let expected_group = self.table.current_group(expected_anchor);
            if expected_group.key == key {
                (expected_anchor, GroupStartKind::Reused)
            } else {
                let search_start = insert_index + expected_group.subtree_len as usize;
                if let Some(found_index) =
                    self.table
                        .find_later_sibling(parent_anchor, key, search_start)
                {
                    let found_anchor = self.table.groups[found_index].anchor;
                    self.table.move_subtree(found_anchor, insert_index);
                    (found_anchor, GroupStartKind::Moved)
                } else {
                    (
                        self.table
                            .insert_new_group(insert_index, parent_anchor, key),
                        GroupStartKind::Inserted,
                    )
                }
            }
        } else {
            (
                self.table
                    .insert_new_group(insert_index, parent_anchor, key),
                GroupStartKind::Inserted,
            )
        };

        self.open_started_group(anchor, kind)
    }

    pub(crate) fn finish_group_body(&mut self) -> FinishGroupResult {
        self.table
            .finish_group_body_internal(self.lifecycle, self.state)
    }

    pub(crate) fn end_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .pop()
            .expect("unbalanced group stack");
        let group_index = self.table.current_group_index(frame.group_anchor);
        let subtree_end = group_index + self.table.groups[group_index].subtree_len as usize;
        if let Some(parent) = self.state.group_stack.last_mut() {
            parent.next_child_index = subtree_end;
        } else {
            self.state.root.next_child_index = subtree_end;
        }
    }

    pub(crate) fn skip_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("skip_group requires an active group");
        let group_index = self.table.current_group_index(frame.group_anchor);
        frame.next_child_index = group_index + self.table.groups[group_index].subtree_len as usize;
        frame.payload_cursor = frame.old_payload_len;
        frame.node_cursor = frame.old_node_len;
        frame.was_skipped = true;
    }

    pub(crate) fn set_group_scope(&mut self, group: GroupId, scope_id: ScopeId) {
        self.table.assign_group_scope(group, scope_id);
    }

    pub(crate) fn end_recompose(&mut self) {
        self.end_group();
    }

    pub(crate) fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> ValueSlotId {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("value slots require an active group");
        let group_anchor = frame.group_anchor;
        let group_index = self.table.current_group_index(group_anchor);
        let payload_len = self.table.group_payload_len_at(group_index);

        let (anchor, generation) = if frame.payload_cursor < payload_len {
            let (anchor, generation) = self
                .table
                .payload_slot_identity_at(group_index, frame.payload_cursor);
            if self
                .table
                .payload_value_is::<T>(group_index, frame.payload_cursor)
            {
                (anchor, generation)
            } else {
                let (old_kind, old_value) =
                    self.table
                        .replace_payload_value(group_index, frame.payload_cursor, init());
                self.lifecycle
                    .queue_drop(DeferredDrop::payload(old_kind, old_value));
                (anchor, generation)
            }
        } else {
            let generation = 1;
            let anchor = self.table.insert_value_payload(
                group_anchor,
                frame.payload_cursor,
                generation,
                init(),
            );
            (anchor, generation)
        };

        let slot = ValueSlotId::new(anchor, generation);
        frame.payload_cursor += 1;
        slot
    }

    pub(crate) fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let slot = self.value_slot(|| Owned::new(init()));
        self.table.read_value::<Owned<T>>(slot).clone()
    }

    pub(crate) fn record_node(&mut self, id: NodeId, generation: u32) -> NodeRecordResult {
        self.record_node_with_parent(id, generation, None)
    }

    pub(crate) fn record_node_with_parent(
        &mut self,
        id: NodeId,
        generation: u32,
        parent_id: Option<NodeId>,
    ) -> NodeRecordResult {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("node records require an active group");
        let group_anchor = frame.group_anchor;
        let group_index = self.table.current_group_index(group_anchor);
        let recorded = self.table.record_group_node(
            group_index,
            frame.node_cursor,
            group_anchor,
            id,
            parent_id,
            generation,
        );
        if !recorded.reused_slot {
            self.table.adjust_ancestor_node_counts(group_anchor, 1);
        }

        frame.node_cursor += 1;
        NodeRecordResult {
            reused: recorded.reused_node,
            id,
        }
    }

    pub(crate) fn current_node_record(&self) -> Option<(NodeId, u32)> {
        let frame = self.state.group_stack.last()?;
        let group_index = self.table.current_group_index(frame.group_anchor);
        if frame.node_cursor >= self.table.group_node_len_at(group_index) {
            return None;
        }
        let node = self
            .table
            .group_node_record_at(group_index, frame.node_cursor);
        Some((node.id, node.generation))
    }

    pub(crate) fn nodes_in_current_group(&self) -> Vec<NodeId> {
        let frame = self
            .state
            .group_stack
            .last()
            .expect("nodes_in_current_group requires an active group");
        self.table.collect_subtree_node_ids(frame.group_anchor)
    }

    pub(crate) fn finalize_pass(&mut self, applier: &mut dyn Applier) -> Vec<DetachedSubtree> {
        while !self.state.group_stack.is_empty() {
            let result = self.finish_group_body();
            for subtree in result.detached_children {
                self.table.invalidate_detached_subtree_anchors(&subtree);
                dispose_detached_subtree_now(applier, &subtree);
                self.lifecycle.queue_subtree_disposal(subtree);
            }
            for node_id in result.direct_nodes {
                let _ = dispose_detached_node_now(applier, node_id);
            }
            self.end_group();
        }

        let root_detached = self.table.root_finish_result(self.state);
        self.state.note_detached_subtrees(&root_detached);
        root_detached
    }
}
