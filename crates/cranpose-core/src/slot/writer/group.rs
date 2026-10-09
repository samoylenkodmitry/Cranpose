use super::{
    super::{
        ActiveGroupId, ActiveSubtreeRoot, ChildCursor, DetachedSubtree, GroupKey, GroupKeySeed,
        GroupStart, GroupStartKind, SlotTable, SlotWriteSession,
    },
    SlotWriteSessionState,
};
use crate::{AnchorId, NodeId, RecomposeScope};

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
    fn open_group_frame(
        &mut self,
        state: &mut SlotWriteSessionState,
        anchor: AnchorId,
        index_hint: usize,
    ) -> Option<usize> {
        let Some(group_index) = self
            .open_group_index(anchor, index_hint)
            .or_else(|| self.recover_group_index_from_recorded_anchor(anchor))
        else {
            log::error!("slot writer could not open group frame for inactive anchor {anchor:?}");
            return None;
        };
        state.push_group_frame(
            anchor,
            group_index,
            self.group_payload_len_at(group_index),
            self.group_node_len_at(group_index),
        );
        Some(group_index)
    }
}

impl SlotWriteSession<'_> {
    pub(crate) fn preview_group_key(&mut self, seed: GroupKeySeed) -> GroupKey {
        self.state.preview_group_key(seed)
    }

    #[inline(always)]
    fn open_started_group(
        &mut self,
        anchor: AnchorId,
        index_hint: usize,
        kind: GroupStartKind,
    ) -> Option<GroupStart<ActiveGroupId>> {
        let group_index = self
            .table
            .open_group_frame(self.state, anchor, index_hint)?;
        let scope = self.table.group_scope_at_index(group_index);
        let Some(group) = self.table.active_group_id_at_index(group_index) else {
            log::error!(
                "slot writer could not create active group handle for group index {group_index}"
            );
            return None;
        };
        Some(GroupStart {
            group,
            anchor,
            scope,
            kind,
        })
    }

    fn open_parent_index(&self) -> Option<usize> {
        let frame = self.state.group_stack.last()?;
        self.table
            .open_group_index(frame.group_anchor, frame.group_index)
    }

    fn discard_stale_group_frames(&mut self) {
        while let Some((group_anchor, group_index)) = self
            .state
            .group_stack
            .last()
            .map(|frame| (frame.group_anchor, frame.group_index))
        {
            if self
                .table
                .open_group_index(group_anchor, group_index)
                .is_some()
            {
                return;
            }
            log::error!(
                "slot writer discarded stale group frame before beginning a child for anchor {group_anchor:?}"
            );
            if self.state.pop_group_frame().is_none() {
                return;
            }
        }
    }

    #[inline(always)]
    fn restore_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
        parent_node: Option<NodeId>,
    ) -> Option<GroupStart<ActiveGroupId>> {
        self.reattach_started_group(key, detached, parent_node, GroupStartKind::Restored)
    }

    fn reattach_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
        parent_node: Option<NodeId>,
        kind: GroupStartKind,
    ) -> Option<GroupStart<ActiveGroupId>> {
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();
        let cursor = ChildCursor::new(parent_anchor, insert_index);
        match self
            .table
            .restore_subtree(cursor, key, detached, parent_node)
        {
            Ok(anchor) => self.open_started_group(anchor, insert_index, kind),
            Err(detached) => {
                log::error!(
                    "slot writer rejected detached subtree restore at parent={parent_anchor:?} child_index={insert_index}"
                );
                self.state.queue_rejected_restore_subtree(detached);
                None
            }
        }
    }

    pub(crate) fn reserve_group_key(&mut self, seed: GroupKeySeed) -> GroupKey {
        self.preview_group_key(seed)
    }

    fn recover_malformed_group_start(
        &mut self,
        key: GroupKey,
        rejected_anchor: AnchorId,
    ) -> GroupStart<ActiveGroupId> {
        self.discard_stale_group_frames();
        let parent_anchor = self.state.current_parent_anchor();
        let siblings = self.table.direct_child_range(parent_anchor);
        let insert_index = siblings.start();
        log::error!(
            "slot writer recovered malformed group start for key {key:?}; rejected_anchor={rejected_anchor:?} parent={parent_anchor:?} fallback_child_index={insert_index}"
        );
        self.state.advance_parent_after_child(insert_index);
        let fallback_cursor = ChildCursor::new(parent_anchor, insert_index);
        let anchor = self.table.insert_new_group(fallback_cursor, key);
        if let Some(started) =
            self.open_started_group(anchor, insert_index, GroupStartKind::Inserted)
        {
            return started;
        }

        while self.state.pop_group_frame().is_some() {}
        let root_insert_index = self.table.group_count();
        self.state.advance_parent_after_child(root_insert_index);
        let root_anchor = self
            .table
            .insert_new_group(ChildCursor::new(AnchorId::INVALID, root_insert_index), key);
        if let Some(started) =
            self.open_started_group(root_anchor, root_insert_index, GroupStartKind::Inserted)
        {
            return started;
        }

        log::error!(
            "slot writer could not recover malformed group start for key {key:?}; returning inert group handle"
        );
        GroupStart {
            group: ActiveGroupId::new(0, 0),
            anchor: AnchorId::INVALID,
            scope: None,
            kind: GroupStartKind::Inserted,
        }
    }

    pub(crate) fn retained_restore_ready(
        &mut self,
        key: GroupKey,
        subtree: &mut DetachedSubtree,
    ) -> bool {
        if !self.table.holds_anchors_of(subtree) && !self.table.adopt_detached_subtree(subtree) {
            log::error!(
                "slot table could not issue its own anchors for a subtree arriving from another slot table for key {key:?}"
            );
            return false;
        }
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();
        let cursor = ChildCursor::new(parent_anchor, insert_index);
        self.table.subtree_restore_ready(cursor, key, subtree)
    }

    #[inline(always)]
    fn resolve_active_child(
        &mut self,
        cursor: ChildCursor,
        key: GroupKey,
    ) -> ActiveChildResolution {
        let siblings = match self.open_parent_index() {
            Some(parent_index) => self
                .table
                .direct_child_range_at(cursor.parent(), parent_index),
            None => self.table.direct_child_range(cursor.parent()),
        };
        let Some(expected_group) = self.table.direct_child_sibling_record_in(siblings, cursor)
        else {
            return ActiveChildResolution::InsertNew;
        };

        if expected_group.key == key {
            return ActiveChildResolution::ReuseExpected {
                anchor: expected_group.anchor,
            };
        }

        let search_start = self.group_subtree_end(cursor.index(), "later sibling search start");
        self.state
            .find_later_sibling(self.table, cursor.parent(), key, search_start)
            .map_or(ActiveChildResolution::InsertNew, |root| {
                ActiveChildResolution::MoveLaterSibling { root }
            })
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

    pub(crate) fn begin_recompose_at_scope(
        &mut self,
        scope: &RecomposeScope,
    ) -> Option<ActiveGroupId> {
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("begin_recompose_at_scope");
        let group = self.table.active_group_for_scope(scope)?;
        let anchor = self.table.try_active_group_anchor(group)?;
        self.table
            .open_group_frame(self.state, anchor, group.index())?;
        Some(group)
    }

    /// The first root node of the open group's subtree.
    pub(crate) fn open_group_first_root_node(&self) -> Option<NodeId> {
        let group_index = self.open_parent_index()?;
        self.table.first_subtree_root_node_id_at(group_index)
    }

    pub(crate) fn begin_group(
        &mut self,
        key: GroupKey,
        restored: Option<Box<DetachedSubtree>>,
        parent_node: Option<NodeId>,
    ) -> GroupStart<ActiveGroupId> {
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("begin_group");
        self.discard_stale_group_frames();
        let cursor = ChildCursor::new(
            self.state.current_parent_anchor(),
            self.state.current_child_cursor(),
        );
        let resolution = restored
            .is_none()
            .then(|| self.resolve_active_child(cursor, key));

        self.state.consume_group_key(key);

        if let Some(restored) = restored
            && let Some(started) = self.restore_started_group(key, *restored, parent_node)
        {
            return started;
        }

        let resolution = resolution.unwrap_or_else(|| self.resolve_active_child(cursor, key));
        let started = self.materialize_group_at_cursor(cursor, key, resolution);
        self.open_started_group(started.anchor, cursor.index(), started.kind)
            .unwrap_or_else(|| self.recover_malformed_group_start(key, started.anchor))
    }

    pub(crate) fn end_group(&mut self) {
        let Some((group_anchor, opened_index)) = self.state.pop_group_frame() else {
            log::error!("slot writer end_group called with an empty group stack");
            return;
        };
        let Some(group_index) = self.table.open_group_index(group_anchor, opened_index) else {
            log::error!("slot writer end_group ignored stale group frame anchor {group_anchor:?}");
            return;
        };
        let subtree_end = self.group_subtree_end(group_index, "group end cursor advance");
        self.state.advance_parent_after_child(subtree_end);
    }

    pub(crate) fn skip_group(&mut self) {
        let Some((group_anchor, opened_index)) = self
            .state
            .group_stack
            .last()
            .map(|frame| (frame.group_anchor, frame.group_index))
        else {
            log::error!("slot writer skip_group called with an empty group stack");
            return;
        };
        let Some(group_index) = self.table.open_group_index(group_anchor, opened_index) else {
            log::error!("slot writer skip_group ignored stale group frame anchor {group_anchor:?}");
            return;
        };
        let subtree_end = self.group_subtree_end(group_index, "group skip cursor advance");
        let Some(frame) = self.state.group_stack.last_mut() else {
            log::error!("slot writer skip_group lost its active group frame before cursor advance");
            return;
        };
        frame.skip_to_existing_group_end(subtree_end);
    }

    pub(crate) fn set_group_scope(&mut self, group: ActiveGroupId, scope: RecomposeScope) -> bool {
        self.table.assign_active_group_scope(group, scope)
    }

    pub(crate) fn end_recompose(&mut self) {
        self.end_group();
    }

    fn group_subtree_end(&self, group_index: usize, operation: &'static str) -> usize {
        self.table.group_subtree_range_at_index(group_index).map_or_else(
            || {
                let fallback_end = group_index.saturating_add(1).min(self.table.group_count());
                log::error!(
                    "slot writer found the stored span of group index {group_index} outside the active groups during {operation}; using fallback end {fallback_end}"
                );
                fallback_end
            },
            |range| range.as_group_range().end(),
        )
    }
}
