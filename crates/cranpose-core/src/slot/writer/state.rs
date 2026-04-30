use super::super::{DetachedSubtree, SlotPassMode, SlotTable};
use super::frames::{GroupFrame, RootFrame};
use crate::{
    collections::map::{HashMap, HashSet},
    AnchorId,
};

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    pub(in crate::slot) root: RootFrame,
    pub(in crate::slot) group_stack: Vec<GroupFrame>,
    payload_location_refreshes: HashMap<AnchorId, usize>,
    pub(in crate::slot) removed_payload_count: usize,
    pub(in crate::slot) removed_node_count: usize,
    pub(in crate::slot) removed_group_count: usize,
    pub(crate) request_compaction: bool,
    pub(crate) request_anchor_storage_compaction: bool,
    pub(crate) request_payload_storage_compaction: bool,
}

impl SlotWriteSessionState {
    pub(in crate::slot) const COMPACT_PAYLOAD_THRESHOLD: usize = 16 * 1024;
    const COMPACT_NODE_THRESHOLD: usize = 16 * 1024;
    const COMPACT_GROUP_THRESHOLD: usize = 32 * 1024;

    pub(crate) fn reset_for_pass(&mut self, mode: SlotPassMode) {
        self.root = RootFrame {
            next_child_index: 0,
            detach_remaining_children: matches!(mode, SlotPassMode::Compose),
            key_ordinals: HashMap::default(),
            seen_group_keys: HashSet::default(),
            sibling_index: None,
        };
        self.group_stack.clear();
        self.payload_location_refreshes.clear();
        self.removed_payload_count = 0;
        self.removed_node_count = 0;
        self.removed_group_count = 0;
        self.request_compaction = false;
        self.request_anchor_storage_compaction = false;
        self.request_payload_storage_compaction = false;
    }

    pub(in crate::slot) fn note_removed_payloads(&mut self, count: usize) {
        self.removed_payload_count += count;
        self.update_compaction_hint();
    }

    pub(in crate::slot) fn note_payload_location_refresh(&mut self, owner: AnchorId, start: usize) {
        self.payload_location_refreshes
            .entry(owner)
            .and_modify(|current| *current = (*current).min(start))
            .or_insert(start);
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn has_pending_payload_location_refreshes(&self) -> bool {
        !self.payload_location_refreshes.is_empty()
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn pending_payload_location_refresh_count(&self) -> usize {
        self.payload_location_refreshes.len()
    }

    pub(crate) fn flush_payload_location_refreshes(&mut self, table: &mut SlotTable) {
        table.flush_payload_location_refreshes(self);
        #[cfg(any(test, debug_assertions))]
        self.debug_assert_no_pending_payload_location_refreshes("writer payload refresh flush");
    }

    #[cfg(test)]
    pub(in crate::slot) fn pending_payload_location_refresh_start(
        &self,
        owner: AnchorId,
    ) -> Option<usize> {
        self.payload_location_refreshes.get(&owner).copied()
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn debug_assert_no_pending_payload_location_refreshes(
        &self,
        operation: &'static str,
    ) {
        debug_assert!(
            !self.has_pending_payload_location_refreshes(),
            "payload location refreshes must be flushed after {operation}"
        );
    }

    pub(in crate::slot) fn drain_payload_location_refreshes(
        &mut self,
    ) -> impl Iterator<Item = (AnchorId, usize)> + '_ {
        self.payload_location_refreshes.drain()
    }

    pub(in crate::slot) fn note_removed_nodes(&mut self, count: usize) {
        self.removed_node_count += count;
        self.update_compaction_hint();
    }

    pub(in crate::slot) fn note_detached_subtrees(&mut self, subtrees: &[DetachedSubtree]) {
        self.removed_group_count += subtrees
            .iter()
            .map(DetachedSubtree::group_count)
            .sum::<usize>();
        self.removed_payload_count += subtrees
            .iter()
            .map(DetachedSubtree::payload_count)
            .sum::<usize>();
        self.removed_node_count += subtrees
            .iter()
            .map(DetachedSubtree::node_count)
            .sum::<usize>();
        self.update_compaction_hint();
    }

    fn update_compaction_hint(&mut self) {
        let payload_pressure = self.removed_payload_count >= Self::COMPACT_PAYLOAD_THRESHOLD;
        let node_pressure = self.removed_node_count >= Self::COMPACT_NODE_THRESHOLD;
        let group_pressure = self.removed_group_count >= Self::COMPACT_GROUP_THRESHOLD;
        self.request_compaction |= payload_pressure || node_pressure || group_pressure;
        self.request_anchor_storage_compaction |= group_pressure;
        self.request_payload_storage_compaction |= payload_pressure;
    }

    pub(in crate::slot) fn current_parent_anchor(&self) -> AnchorId {
        self.group_stack
            .last()
            .map(|frame| frame.group_anchor)
            .unwrap_or(AnchorId::INVALID)
    }

    pub(in crate::slot) fn current_child_cursor(&self) -> usize {
        if let Some(frame) = self.group_stack.last() {
            frame.next_child_index
        } else {
            self.root.next_child_index
        }
    }

    pub(in crate::slot) fn advance_parent_after_child(&mut self, subtree_end: usize) {
        if let Some(parent) = self.group_stack.last_mut() {
            parent.next_child_index = subtree_end;
        } else {
            self.root.next_child_index = subtree_end;
        }
    }

    pub(in crate::slot) fn push_group_frame(
        &mut self,
        anchor: AnchorId,
        next_child_index: usize,
        old_payload_len: usize,
        old_node_len: usize,
    ) {
        self.group_stack.push(GroupFrame {
            group_anchor: anchor,
            next_child_index,
            payload_cursor: 0,
            old_payload_len,
            node_cursor: 0,
            old_node_len,
            key_ordinals: HashMap::default(),
            seen_group_keys: HashSet::default(),
            sibling_index: None,
            body_finished: false,
            was_skipped: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::{GroupKeySeed, SlotInvariantError, SlotLifecycleCoordinator, SlotTable};

    #[test]
    fn removed_payloads_trigger_compaction_hint_at_threshold() {
        let mut state = SlotWriteSessionState::default();

        state.note_removed_payloads(SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD - 1);
        assert!(!state.request_compaction);
        assert!(!state.request_payload_storage_compaction);

        state.note_removed_payloads(1);
        assert!(state.request_compaction);
        assert!(state.request_payload_storage_compaction);
        assert!(!state.request_anchor_storage_compaction);
    }

    #[test]
    fn group_removal_requests_anchor_storage_compaction() {
        let mut state = SlotWriteSessionState {
            removed_group_count: SlotWriteSessionState::COMPACT_GROUP_THRESHOLD,
            ..Default::default()
        };
        state.update_compaction_hint();

        assert!(state.request_compaction);
        assert!(state.request_anchor_storage_compaction);
        assert!(!state.request_payload_storage_compaction);
    }

    #[test]
    fn node_removal_compaction_does_not_request_storage_cleanup() {
        let mut state = SlotWriteSessionState {
            removed_node_count: SlotWriteSessionState::COMPACT_NODE_THRESHOLD,
            ..Default::default()
        };
        state.update_compaction_hint();

        assert!(state.request_compaction);
        assert!(!state.request_anchor_storage_compaction);
        assert!(!state.request_payload_storage_compaction);
    }

    #[test]
    fn validate_reports_writer_frame_out_of_bounds() {
        let mut table = SlotTable::new();
        let mut lifecycle = SlotLifecycleCoordinator::default();
        let mut state = SlotWriteSessionState::default();
        state.reset_for_pass(SlotPassMode::Compose);

        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(key, None);
        }

        state.group_stack[0].next_child_index = table.group_count() + 1;

        assert_eq!(
            state.validate(&table),
            Err(SlotInvariantError::WriterFrameOutOfBounds {
                frame_index: 1,
                group_anchor: table.group_anchor_at_index(0),
                field: "next_child_index",
                value: 2,
                min: 1,
                max: 1,
            })
        );
    }

    #[test]
    fn validate_reports_writer_frame_not_at_direct_child_boundary() {
        let mut table = SlotTable::new();
        let mut lifecycle = SlotLifecycleCoordinator::default();
        let mut state = SlotWriteSessionState::default();
        state.reset_for_pass(SlotPassMode::Compose);

        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let root_key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(root_key, None);

            let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
            let _ = session.begin_group(child_key, None);

            let grandchild_key = session.preview_group_key(GroupKeySeed::unkeyed(12));
            let _ = session.begin_group(grandchild_key, None);
        }

        let root_anchor = table.group_anchor_at_index(0);
        let child_anchor = table.group_anchor_at_index(1);
        state.group_stack[0].next_child_index = 2;

        assert_eq!(
            state.validate(&table),
            Err(SlotInvariantError::WriterFrameNotAtChildBoundary {
                frame_index: 1,
                group_anchor: root_anchor,
                next_child_index: 2,
                expected_parent: root_anchor,
                actual_parent: child_anchor,
            })
        );
    }

    #[test]
    fn validate_allows_growing_payload_and_node_cursors() {
        let mut table = SlotTable::new();
        let mut lifecycle = SlotLifecycleCoordinator::default();
        let mut state = SlotWriteSessionState::default();
        state.reset_for_pass(SlotPassMode::Compose);

        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(key, None);
            let _ = session.value_slot_with_kind(crate::slot::PayloadKind::Internal, || 17_i32);
            session.record_node_with_parent(31, 1, None);
        }

        assert_eq!(state.group_stack[0].old_payload_len, 0);
        assert_eq!(state.group_stack[0].payload_cursor, 1);
        assert_eq!(state.group_stack[0].old_node_len, 0);
        assert_eq!(state.group_stack[0].node_cursor, 1);
        state.flush_payload_location_refreshes(&mut table);
        assert_eq!(state.validate(&table), Ok(()));
    }

    #[test]
    fn validate_allows_scoped_recompose_root_depth() {
        let mut table = SlotTable::new();
        let mut lifecycle = SlotLifecycleCoordinator::default();
        let mut state = SlotWriteSessionState::default();
        let scope_id = 41;

        state.reset_for_pass(SlotPassMode::Compose);
        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let root_key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(root_key, None);

            let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
            let child = session.begin_group(child_key, None);
            session.set_group_scope(child.group, scope_id);
            let _ = session.finish_group_body();
            session.end_group();

            let _ = session.finish_group_body();
            session.end_group();
        }

        state.reset_for_pass(SlotPassMode::Recompose);
        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let _ = session
                .begin_recompose_at_scope(scope_id)
                .expect("scoped recompose should resolve");
        }

        assert_eq!(state.group_stack.len(), 1);
        state.flush_payload_location_refreshes(&mut table);
        assert_eq!(state.validate(&table), Ok(()));
    }

    #[test]
    fn validate_allows_finished_frame_after_direct_node_removal() {
        let mut table = SlotTable::new();
        let mut lifecycle = SlotLifecycleCoordinator::default();
        let mut state = SlotWriteSessionState::default();

        state.reset_for_pass(SlotPassMode::Compose);
        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(key, None);
            session.record_node_with_parent(31, 1, None);
            let _ = session.finish_group_body();
            session.end_group();
        }

        state.reset_for_pass(SlotPassMode::Compose);
        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(key, None);
            let _ = session.finish_group_body();
        }

        assert_eq!(state.group_stack[0].old_node_len, 1);
        assert_eq!(table.group_node_len_at(0), 0);
        state.flush_payload_location_refreshes(&mut table);
        assert_eq!(state.validate(&table), Ok(()));
    }
}
