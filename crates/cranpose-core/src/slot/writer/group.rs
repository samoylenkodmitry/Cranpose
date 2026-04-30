use super::super::{
    ActiveGroupId, ActiveSubtreeRoot, ChildCursor, DetachedSubtree, GroupKey, GroupKeySeed,
    GroupStart, GroupStartKind, SlotTable, SlotWriteSession,
};
use super::SlotWriteSessionState;
use crate::{AnchorId, ScopeId};

enum ActiveChildResolution {
    ReuseExpected { anchor: AnchorId },
    MoveLaterSibling { root: ActiveSubtreeRoot },
    InsertNew,
}

struct StartedGroup {
    anchor: AnchorId,
    kind: GroupStartKind,
}

impl SlotTable {
    fn open_group_frame(&mut self, state: &mut SlotWriteSessionState, anchor: AnchorId) -> usize {
        let group_index = self.current_group_index(anchor);
        state.push_group_frame(
            anchor,
            group_index + 1,
            self.group_payload_len_at(group_index),
            self.group_node_len_at(group_index),
        );
        group_index
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn preview_group_key(&self, seed: GroupKeySeed) -> GroupKey {
        self.state.preview_group_key(seed)
    }

    #[inline(always)]
    fn open_started_group(
        &mut self,
        anchor: AnchorId,
        kind: GroupStartKind,
    ) -> GroupStart<ActiveGroupId> {
        let group_index = self.table.open_group_frame(self.state, anchor);
        let scope_id = self.table.group_scope_id_at_index(group_index);
        GroupStart {
            group: self.table.active_group_id_at_index(group_index),
            #[cfg(test)]
            anchor,
            scope_id,
            kind,
        }
    }

    #[inline(always)]
    fn restore_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
    ) -> GroupStart<ActiveGroupId> {
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();
        let cursor = ChildCursor::new(parent_anchor, insert_index);
        let anchor = self.table.restore_subtree(cursor, key, detached);
        self.open_started_group(anchor, GroupStartKind::Restored)
    }

    pub(crate) fn assert_retained_restore_ready(
        &mut self,
        key: GroupKey,
        subtree: &DetachedSubtree,
    ) {
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();
        let cursor = ChildCursor::new(parent_anchor, insert_index);
        self.table
            .assert_subtree_restore_ready(cursor, key, subtree);
    }

    #[inline(always)]
    fn resolve_active_child(
        &mut self,
        cursor: ChildCursor,
        key: GroupKey,
    ) -> ActiveChildResolution {
        let Some(expected_group) = self.table.direct_child_sibling_record_at_cursor(cursor) else {
            return ActiveChildResolution::InsertNew;
        };

        if expected_group.key == key {
            return ActiveChildResolution::ReuseExpected {
                anchor: expected_group.anchor,
            };
        }

        let search_start = cursor.index() + expected_group.subtree_len;
        self.state
            .find_later_sibling(self.table, cursor.parent(), key, search_start)
            .map(|root| ActiveChildResolution::MoveLaterSibling { root })
            .unwrap_or(ActiveChildResolution::InsertNew)
    }

    #[inline(always)]
    fn materialize_group_at_cursor(
        &mut self,
        cursor: ChildCursor,
        key: GroupKey,
        resolution: ActiveChildResolution,
    ) -> StartedGroup {
        match resolution {
            ActiveChildResolution::ReuseExpected { anchor } => StartedGroup {
                anchor,
                kind: GroupStartKind::Reused,
            },
            ActiveChildResolution::MoveLaterSibling { root } => {
                self.table
                    .move_later_sibling_subtree_to_cursor(root, cursor);
                StartedGroup {
                    anchor: root.anchor(),
                    kind: GroupStartKind::Moved,
                }
            }
            ActiveChildResolution::InsertNew => StartedGroup {
                anchor: self.table.insert_new_group(cursor, key),
                kind: GroupStartKind::Inserted,
            },
        }
    }

    pub(crate) fn begin_recompose_at_scope(&mut self, scope_id: ScopeId) -> Option<ActiveGroupId> {
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("begin_recompose_at_scope");
        let group = self.table.active_group_for_scope(scope_id)?;
        let anchor = self.table.active_group_anchor(group);
        self.table.open_group_frame(self.state, anchor);
        Some(group)
    }

    pub(crate) fn begin_group(
        &mut self,
        key: GroupKey,
        restored: Option<DetachedSubtree>,
    ) -> GroupStart<ActiveGroupId> {
        self.state.consume_group_key(key);
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("begin_group");
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();

        if let Some(restored) = restored {
            return self.restore_started_group(key, restored);
        }

        let cursor = ChildCursor::new(parent_anchor, insert_index);
        let resolution = self.resolve_active_child(cursor, key);
        let started = self.materialize_group_at_cursor(cursor, key, resolution);
        self.open_started_group(started.anchor, started.kind)
    }

    pub(crate) fn end_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .pop()
            .expect("unbalanced group stack");
        let group_index = self.table.current_group_index(frame.group_anchor);
        let subtree_end = self.table.group_subtree_end_at_index(group_index);
        self.state.advance_parent_after_child(subtree_end);
    }

    pub(crate) fn skip_group(&mut self) {
        let frame = self
            .state
            .group_stack
            .last_mut()
            .expect("skip_group requires an active group");
        let group_index = self.table.current_group_index(frame.group_anchor);
        frame.skip_to_existing_group_end(
            group_index,
            self.table.group_subtree_len_at_index(group_index),
        );
    }

    pub(crate) fn set_group_scope(&mut self, group: ActiveGroupId, scope_id: ScopeId) {
        self.table.assign_active_group_scope(group, scope_id);
    }

    pub(crate) fn end_recompose(&mut self) {
        self.end_group();
    }
}
