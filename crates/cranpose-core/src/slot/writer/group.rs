use super::{
    super::{
        ActiveGroupId, ActiveSubtreeRoot, ChildCursor, DetachedSubtree, GroupKey, GroupKeySeed,
        GroupStart, GroupStartKind, SlotTable, SlotWriteSession,
    },
    SlotWriteSessionState,
};
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
    fn open_group_frame(
        &mut self,
        state: &mut SlotWriteSessionState,
        anchor: AnchorId,
    ) -> Option<usize> {
        let Some(group_index) = self
            .active_group_index(anchor)
            .or_else(|| self.recover_group_index_from_recorded_anchor(anchor))
        else {
            log::error!("slot writer could not open group frame for inactive anchor {anchor:?}");
            return None;
        };
        state.push_group_frame(
            anchor,
            group_index + 1,
            self.group_payload_len_at(group_index),
            self.group_node_len_at(group_index),
        );
        Some(group_index)
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
    ) -> Option<GroupStart<ActiveGroupId>> {
        let group_index = self.table.open_group_frame(self.state, anchor)?;
        let scope_id = self.table.group_scope_id_at_index(group_index);
        let Some(group) = self.table.active_group_id_at_index(group_index) else {
            log::error!(
                "slot writer could not create active group handle for group index {group_index}"
            );
            return None;
        };
        Some(GroupStart {
            group,
            anchor,
            scope_id,
            kind,
        })
    }

    fn discard_stale_group_frames(&mut self) {
        while let Some(group_anchor) = self
            .state
            .group_stack
            .last()
            .map(|frame| frame.group_anchor)
        {
            if self.table.active_group_index(group_anchor).is_some() {
                return;
            }
            log::error!(
                "slot writer discarded stale group frame before beginning a child for anchor {group_anchor:?}"
            );
            let Some(frame) = self.state.group_stack.pop() else {
                return;
            };
            self.state.recycle_group_frame(frame);
        }
    }

    #[inline(always)]
    fn restore_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
    ) -> Option<GroupStart<ActiveGroupId>> {
        self.reattach_started_group(key, detached, GroupStartKind::Restored)
    }

    /// Bring a keyed subtree back that never left the composition's applier —
    /// it moved between branch brackets within this pass. `Moved` rather than
    /// `Restored`: the content's scopes are live and its nodes attached, so
    /// the normal skip machinery re-registers them without a forced
    /// recomposition.
    fn adopt_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
    ) -> Option<GroupStart<ActiveGroupId>> {
        self.reattach_started_group(key, detached, GroupStartKind::Moved)
    }

    fn reattach_started_group(
        &mut self,
        key: GroupKey,
        detached: DetachedSubtree,
        kind: GroupStartKind,
    ) -> Option<GroupStart<ActiveGroupId>> {
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();
        let cursor = ChildCursor::new(parent_anchor, insert_index);
        match self.table.restore_subtree(cursor, key, detached) {
            Ok(anchor) => self.open_started_group(anchor, kind),
            Err(detached) => {
                log::error!(
                    "slot writer rejected detached subtree restore at parent={parent_anchor:?} child_index={insert_index}"
                );
                self.state.queue_rejected_restore_subtree(detached);
                None
            }
        }
    }

    /// Keyed continuity across branch brackets of one branch site: an
    /// explicit key that would otherwise insert fresh may claim the identical
    /// key parked by an already-finished bracket of the same site and owner,
    /// or steal it out of a later same-site bracket that has not composed
    /// yet. Both are the same shifted-by-one list; without this, hiding one
    /// row rebuilds every keyed row behind it and loses their state. A key
    /// arriving under a *different* branch site — or under no bracket at
    /// all — gets neither: branch isolation wins over the key.
    fn claim_or_steal_keyed_subtree(&mut self, key: GroupKey) -> Option<DetachedSubtree> {
        key.explicit_key?;
        let parent_anchor = self.state.current_parent_anchor();
        let branch_site = self
            .table
            .active_group_index(parent_anchor)
            .and_then(|index| self.table.groups.get(index))
            .filter(|group| group.transparent)
            .map(|group| group.key.static_key)?;
        if self.state.has_orphaned_keyed() {
            let owner = self.table.nearest_non_transparent_ancestor(parent_anchor);
            if let Some(subtree) = self.state.claim_orphaned_keyed(owner, branch_site, key) {
                return Some(subtree);
            }
        }
        self.table
            .steal_keyed_subtree_from_later_transparent_siblings(parent_anchor, key)
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
            "slot writer recovered malformed group start for key {:?}; rejected_anchor={rejected_anchor:?} parent={parent_anchor:?} fallback_child_index={insert_index}",
            key
        );
        self.state.advance_parent_after_child(insert_index);
        let fallback_cursor = ChildCursor::new(parent_anchor, insert_index);
        let anchor = self.table.insert_new_group(fallback_cursor, key);
        if let Some(started) = self.open_started_group(anchor, GroupStartKind::Inserted) {
            return started;
        }

        while let Some(frame) = self.state.group_stack.pop() {
            self.state.recycle_group_frame(frame);
        }
        let root_insert_index = self.table.group_count();
        self.state.advance_parent_after_child(root_insert_index);
        let root_anchor = self
            .table
            .insert_new_group(ChildCursor::new(AnchorId::INVALID, root_insert_index), key);
        if let Some(started) = self.open_started_group(root_anchor, GroupStartKind::Inserted) {
            return started;
        }

        log::error!(
            "slot writer could not recover malformed group start for key {:?}; returning inert group handle",
            key
        );
        GroupStart {
            group: ActiveGroupId::new(0, 0),
            anchor: AnchorId::INVALID,
            scope_id: None,
            kind: GroupStartKind::Inserted,
        }
    }

    pub(crate) fn retained_restore_ready(
        &mut self,
        key: GroupKey,
        subtree: &DetachedSubtree,
    ) -> bool {
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
        let Some(expected_group) = self.table.direct_child_sibling_record_at_cursor(cursor) else {
            return ActiveChildResolution::InsertNew;
        };

        if expected_group.key == key {
            return ActiveChildResolution::ReuseExpected {
                anchor: expected_group.anchor,
            };
        }

        let search_start = self
            .table
            .repair_group_subtree_range_at_index(cursor.index(), "later sibling search start")
            .map(|range| range.as_group_range().end())
            .unwrap_or_else(|| {
                let fallback = cursor.index().saturating_add(1).min(self.table.group_count());
                log::error!(
                    "slot writer could not repair expected child subtree at cursor {:?} before keyed sibling search; using fallback search start {fallback}",
                    cursor
                );
                fallback
            });
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
        let anchor = self.table.try_active_group_anchor(group)?;
        self.table.open_group_frame(self.state, anchor)?;
        Some(group)
    }

    /// Reserve a branch bracket without opening it. An explicitly keyed child
    /// group passes through (it states its own identity); anything else at
    /// the bracket's level first materializes it via
    /// [`Self::materialize_deferred_branch_shells`]. Returns the token the
    /// closing guard hands back to [`Self::close_deferred_branch_shell`].
    pub(crate) fn begin_deferred_branch_shell(&mut self, seed: GroupKeySeed) -> usize {
        let parent = self.state.current_parent_anchor();
        self.state
            .deferred_branch_shells
            .push(super::state::DeferredBranchShell {
                parent,
                seed,
                materialized: false,
            });
        self.state.deferred_branch_shells.len() - 1
    }

    /// Close the shell the guard holds. Returns whether it materialized into
    /// a real open group the caller must now finish like any branch group.
    pub(crate) fn close_deferred_branch_shell(&mut self, token: usize) -> bool {
        if self.state.deferred_branch_shells.len() != token + 1 {
            log::error!(
                "deferred branch shell {token} closed out of order at depth {}",
                self.state.deferred_branch_shells.len()
            );
            return self
                .state
                .deferred_branch_shells
                .get(token)
                .is_some_and(|shell| shell.materialized);
        }
        let shell = self
            .state
            .deferred_branch_shells
            .pop()
            .expect("length checked above");
        shell.materialized
    }

    /// Open every pending branch shell whose level the next operation is
    /// about to write into. Marked materialized before its `begin_group` so
    /// the recursive hook terminates.
    pub(in crate::slot) fn materialize_deferred_branch_shells(&mut self) {
        while let Some(shell) = self.state.deferred_branch_shells.last() {
            if shell.materialized || shell.parent != self.state.current_parent_anchor() {
                return;
            }
            let seed = shell.seed;
            self.state
                .deferred_branch_shells
                .last_mut()
                .expect("checked above")
                .materialized = true;
            let key = self.preview_group_key(seed);
            let started = self.begin_group(key, None);
            self.mark_group_transparent(started.group);
        }
    }

    pub(crate) fn begin_group(
        &mut self,
        key: GroupKey,
        restored: Option<DetachedSubtree>,
    ) -> GroupStart<ActiveGroupId> {
        if key.explicit_key.is_none() {
            self.materialize_deferred_branch_shells();
        }
        self.state.consume_group_key(key);
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("begin_group");
        self.discard_stale_group_frames();
        let parent_anchor = self.state.current_parent_anchor();
        let insert_index = self.state.current_child_cursor();

        if let Some(restored) = restored {
            if let Some(started) = self.restore_started_group(key, restored) {
                return started;
            }
        }

        let cursor = ChildCursor::new(parent_anchor, insert_index);
        let resolution = self.resolve_active_child(cursor, key);
        if matches!(resolution, ActiveChildResolution::InsertNew) {
            if let Some(subtree) = self.claim_or_steal_keyed_subtree(key) {
                if let Some(started) = self.adopt_started_group(key, subtree) {
                    return started;
                }
            }
        }
        let started = self.materialize_group_at_cursor(cursor, key, resolution);
        self.open_started_group(started.anchor, started.kind)
            .unwrap_or_else(|| self.recover_malformed_group_start(key, started.anchor))
    }

    pub(crate) fn end_group(&mut self) {
        let Some(frame) = self.state.group_stack.pop() else {
            log::error!("slot writer end_group called with an empty group stack");
            return;
        };
        let group_anchor = frame.group_anchor;
        while self
            .state
            .deferred_branch_shells
            .last()
            .is_some_and(|shell| !shell.materialized && shell.parent == group_anchor)
        {
            log::error!("a deferred branch shell leaked past its group; dropping it");
            self.state.deferred_branch_shells.pop();
        }
        let Some(group_index) = self.table.active_group_index(group_anchor) else {
            log::error!("slot writer end_group ignored stale group frame anchor {group_anchor:?}");
            self.state.recycle_group_frame(frame);
            return;
        };
        let subtree_end = self.repaired_group_subtree_end(group_index, "group end cursor advance");
        self.state.recycle_group_frame(frame);
        self.state.advance_parent_after_child(subtree_end);
    }

    pub(crate) fn skip_group(&mut self) {
        let Some(group_anchor) = self
            .state
            .group_stack
            .last()
            .map(|frame| frame.group_anchor)
        else {
            log::error!("slot writer skip_group called with an empty group stack");
            return;
        };
        let Some(group_index) = self.table.active_group_index(group_anchor) else {
            log::error!(
                "slot writer skip_group ignored stale group frame anchor {:?}",
                group_anchor
            );
            return;
        };
        let subtree_len = self.repaired_group_subtree_len(group_index, "group skip cursor advance");
        let Some(frame) = self.state.group_stack.last_mut() else {
            log::error!("slot writer skip_group lost its active group frame before cursor advance");
            return;
        };
        frame.skip_to_existing_group_end(group_index, subtree_len);
    }

    pub(crate) fn set_group_scope(&mut self, group: ActiveGroupId, scope_id: ScopeId) -> bool {
        self.table.assign_active_group_scope(group, scope_id)
    }

    pub(crate) fn mark_group_transparent(&mut self, group: ActiveGroupId) -> bool {
        self.table.mark_group_transparent(group)
    }

    pub(crate) fn end_recompose(&mut self) {
        self.end_group();
    }

    fn repaired_group_subtree_len(&mut self, group_index: usize, operation: &'static str) -> usize {
        self.table
            .repair_group_subtree_range_at_index(group_index, operation)
            .map(|range| range.len())
            .unwrap_or_else(|| {
                log::error!(
                    "slot writer could not repair subtree length for group index {group_index} during {operation}; advancing by the root group only"
                );
                1
            })
    }

    fn repaired_group_subtree_end(&mut self, group_index: usize, operation: &'static str) -> usize {
        self.table
            .repair_group_subtree_range_at_index(group_index, operation)
            .map(|range| range.as_group_range().end())
            .unwrap_or_else(|| {
                let fallback_end = group_index.saturating_add(1).min(self.table.group_count());
                log::error!(
                    "slot writer could not repair subtree end for group index {group_index} during {operation}; using fallback end {fallback_end}"
                );
                fallback_end
            })
    }
}
