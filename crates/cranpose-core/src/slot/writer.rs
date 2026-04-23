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
            self.direct_child_anchors(anchor),
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
        let remaining_children = {
            let frame = state
                .group_stack
                .last_mut()
                .expect("detach_unvisited_children requires an active group");
            frame
                .old_children
                .drain(frame.old_cursor..)
                .collect::<Vec<_>>()
        };

        let mut detached_children = Vec::new();
        for anchor in remaining_children.into_iter().rev() {
            if self.anchors.contains_active(anchor) {
                detached_children.push(self.detach_subtree(anchor));
            }
        }
        detached_children.reverse();
        state.note_detached_subtrees(&detached_children);
        detached_children
    }

    fn finish_group_body_internal(
        &mut self,
        lifecycle: &mut SlotLifecycleCoordinator,
        state: &mut SlotWriteSessionState,
    ) -> FinishGroupResult {
        let (group_anchor, payload_cursor, node_cursor, start_kind) = {
            let frame = state
                .group_stack
                .last_mut()
                .expect("finish_group_body requires an active group");
            if frame.body_finished {
                return FinishGroupResult {
                    detached_children: Vec::new(),
                    structure_changed: false,
                    direct_nodes: Vec::new(),
                };
            }

            frame.body_finished = true;
            (
                frame.group_anchor,
                frame.payload_cursor,
                frame.node_cursor,
                frame.start_kind,
            )
        };

        let payload_len = self.group_payload_len_at(self.current_group_index(group_anchor));
        if payload_cursor < payload_len {
            let removed = self.remove_payload_range(group_anchor, payload_cursor, payload_len);
            for payload in removed {
                lifecycle.queue_drop(DeferredDrop::Value(payload.value));
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
        let structure_changed = !matches!(start_kind, GroupStartKind::Reused)
            || !direct_nodes.is_empty()
            || !detached_children.is_empty();
        state.note_removed_nodes(direct_nodes.len());
        FinishGroupResult {
            detached_children,
            structure_changed,
            direct_nodes,
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
        const SIBLING_INDEX_THRESHOLD: usize = 16;

        let BeginGroupInput { key, restored } = input;
        self.state.consume_group_key(key);
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = *self.state.current_next_child_index();

        let (old_children, old_cursor, sibling_index) = self.state.current_child_cursor();
        let mut reused_position = None;
        let mut moved = false;
        if restored.is_none() {
            if let Some(&expected_anchor) = old_children.get(*old_cursor) {
                let expected = self.table.current_group(expected_anchor);
                if expected.parent_anchor == parent_anchor && expected.key == key {
                    reused_position = Some(*old_cursor);
                } else {
                    let later_position = if old_children.len().saturating_sub(*old_cursor + 1)
                        >= SIBLING_INDEX_THRESHOLD
                    {
                        let index = sibling_index.get_or_insert_with(|| {
                            let mut index = HashMap::default();
                            for (position, &anchor) in
                                old_children.iter().enumerate().skip(*old_cursor + 1)
                            {
                                let group = self.table.current_group(anchor);
                                if group.parent_anchor == parent_anchor {
                                    index.entry(group.key).or_insert(position);
                                }
                            }
                            index
                        });
                        index.get(&key).copied()
                    } else {
                        None
                    };
                    let search_start = *old_cursor + 1;
                    let search_position = later_position.or_else(|| {
                        (search_start..old_children.len()).find(|&position| {
                            let anchor = old_children[position];
                            let group = self.table.current_group(anchor);
                            group.parent_anchor == parent_anchor && group.key == key
                        })
                    });
                    if let Some(position) = search_position {
                        let anchor = old_children[position];
                        old_children.remove(position);
                        old_children.insert(*old_cursor, anchor);
                        *sibling_index = None;
                        reused_position = Some(*old_cursor);
                        self.table.move_subtree(anchor, insert_index);
                        moved = true;
                    }
                }
            }
        }

        if let Some(restored) = restored {
            return self.restore_started_group(key, restored);
        }

        let (anchor, kind) = if reused_position.is_some() {
            let anchor = old_children[*old_cursor];
            *old_cursor += 1;
            *sibling_index = None;
            (
                anchor,
                if moved {
                    GroupStartKind::Moved
                } else {
                    GroupStartKind::Reused
                },
            )
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
        frame.old_cursor = frame.old_children.len();
        frame.payload_cursor = frame.old_payload_len;
        frame.node_cursor = frame.old_node_len;
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
                let old_value =
                    self.table
                        .replace_payload_value(group_index, frame.payload_cursor, init());
                self.lifecycle.queue_drop(DeferredDrop::Value(old_value));
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
        let group_index = self.table.current_group_index(frame.group_anchor);
        let subtree_end = group_index + self.table.groups[group_index].subtree_len as usize;
        self.table.groups[group_index..subtree_end]
            .iter()
            .enumerate()
            .flat_map(|(offset, _)| {
                self.table
                    .group_node_records_at(group_index + offset)
                    .iter()
            })
            .map(|node| node.id)
            .collect()
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
                dispose_detached_node_now(applier, node_id);
            }
            self.end_group();
        }

        let root_detached = self.table.root_finish_result(self.state);
        self.state.note_detached_subtrees(&root_detached);
        root_detached
    }
}
