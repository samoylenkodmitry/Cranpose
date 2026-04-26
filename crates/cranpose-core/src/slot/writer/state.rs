use super::frames::{GroupFrame, RootFrame};
#[cfg(any(test, debug_assertions))]
use crate::slot::SlotInvariantError;
#[cfg(any(test, debug_assertions))]
use crate::slot::SlotTable;
use crate::{
    collections::map::{HashMap, HashSet},
    slot::{DetachedSubtree, SlotPassMode},
    AnchorId,
};

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    pub(in crate::slot) root: RootFrame,
    pub(in crate::slot) group_stack: Vec<GroupFrame>,
    pub(in crate::slot) removed_payload_count: usize,
    pub(in crate::slot) removed_node_count: usize,
    pub(in crate::slot) removed_group_count: usize,
    pub(crate) request_compaction: bool,
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
        self.removed_payload_count = 0;
        self.removed_node_count = 0;
        self.removed_group_count = 0;
        self.request_compaction = false;
    }

    #[cfg(any(test, debug_assertions))]
    fn writer_frame_out_of_bounds(
        frame_index: usize,
        group_anchor: AnchorId,
        field: &'static str,
        value: usize,
        min: usize,
        max: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::WriterFrameOutOfBounds {
            frame_index,
            group_anchor,
            field,
            value,
            min,
            max,
        }
    }

    #[cfg(any(test, debug_assertions))]
    fn validate_cursor(
        frame_index: usize,
        group_anchor: AnchorId,
        field: &'static str,
        value: usize,
        min: usize,
        max: usize,
    ) -> Result<(), SlotInvariantError> {
        if value < min || value > max {
            return Err(Self::writer_frame_out_of_bounds(
                frame_index,
                group_anchor,
                field,
                value,
                min,
                max,
            ));
        }
        Ok(())
    }

    #[cfg(any(test, debug_assertions))]
    fn validate_child_boundary(
        table: &SlotTable,
        frame_index: usize,
        group_anchor: AnchorId,
        expected_parent: AnchorId,
        next_child_index: usize,
    ) -> Result<(), SlotInvariantError> {
        if table
            .direct_child_anchor_at(expected_parent, next_child_index)
            .is_some()
            || next_child_index == table.direct_child_range_end(expected_parent)
        {
            return Ok(());
        }

        let actual_parent = table
            .groups
            .get(next_child_index)
            .map(|group| group.parent_anchor)
            .unwrap_or(AnchorId::INVALID);
        Err(SlotInvariantError::WriterFrameNotAtChildBoundary {
            frame_index,
            group_anchor,
            next_child_index,
            expected_parent,
            actual_parent,
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn validate(&self, table: &SlotTable) -> Result<(), SlotInvariantError> {
        table.validate()?;

        Self::validate_cursor(
            0,
            AnchorId::INVALID,
            "root.next_child_index",
            self.root.next_child_index,
            0,
            table.groups.len(),
        )?;
        if self.root.detach_remaining_children {
            Self::validate_child_boundary(
                table,
                0,
                AnchorId::INVALID,
                AnchorId::INVALID,
                self.root.next_child_index,
            )?;
        }

        let mut parent_group_index = None;
        let mut parent_subtree_end = table.groups.len();
        let mut parent_depth = None;

        for (depth, frame) in self.group_stack.iter().enumerate() {
            let frame_index = depth + 1;
            let Some(group_index) = table.anchors.active_index(frame.group_anchor) else {
                return Err(SlotInvariantError::AnchorMismatch {
                    anchor: frame.group_anchor,
                    expected: parent_group_index.unwrap_or(0),
                    actual: table.anchors.state(frame.group_anchor),
                });
            };
            let min_group_index = parent_group_index.map_or(0, |index| index + 1);
            let max_group_index = parent_subtree_end.saturating_sub(1);
            Self::validate_cursor(
                frame_index,
                frame.group_anchor,
                "group_index",
                group_index,
                min_group_index,
                max_group_index,
            )?;

            let group = &table.groups[group_index];
            let payload_len = group.payload_len as usize;
            let node_len = group.node_len as usize;
            let payload_limit = frame.old_payload_len.max(payload_len);
            let node_limit = frame.old_node_len.max(node_len);
            if let Some(parent_depth) = parent_depth {
                let expected_depth = parent_depth + 1;
                Self::validate_cursor(
                    frame_index,
                    frame.group_anchor,
                    "group_depth",
                    group.depth as usize,
                    expected_depth,
                    expected_depth,
                )?;
            }

            let subtree_end = group_index + group.subtree_len as usize;
            Self::validate_cursor(
                frame_index,
                frame.group_anchor,
                "next_child_index",
                frame.next_child_index,
                group_index + 1,
                subtree_end,
            )?;
            Self::validate_child_boundary(
                table,
                frame_index,
                frame.group_anchor,
                frame.group_anchor,
                frame.next_child_index,
            )?;
            Self::validate_cursor(
                frame_index,
                frame.group_anchor,
                "payload_cursor",
                frame.payload_cursor,
                0,
                payload_limit,
            )?;
            Self::validate_cursor(
                frame_index,
                frame.group_anchor,
                "node_cursor",
                frame.node_cursor,
                0,
                node_limit,
            )?;

            parent_group_index = Some(group_index);
            parent_subtree_end = subtree_end;
            parent_depth = Some(group.depth as usize);
        }

        Ok(())
    }

    pub(in crate::slot) fn note_removed_payloads(&mut self, count: usize) {
        self.removed_payload_count += count;
        self.update_compaction_hint();
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
        self.request_compaction |= self.removed_payload_count >= Self::COMPACT_PAYLOAD_THRESHOLD
            || self.removed_node_count >= Self::COMPACT_NODE_THRESHOLD
            || self.removed_group_count >= Self::COMPACT_GROUP_THRESHOLD;
    }

    pub(in crate::slot) fn current_parent_anchor(&self) -> AnchorId {
        self.group_stack
            .last()
            .map(|frame| frame.group_anchor)
            .unwrap_or(AnchorId::INVALID)
    }

    pub(in crate::slot) fn current_next_child_index(&mut self) -> &mut usize {
        if let Some(frame) = self.group_stack.last_mut() {
            &mut frame.next_child_index
        } else {
            &mut self.root.next_child_index
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
    use crate::slot::SlotLifecycleCoordinator;
    use crate::slot_storage::{BeginGroupInput, GroupKeySeed};

    #[test]
    fn removed_payloads_trigger_compaction_hint_at_threshold() {
        let mut state = SlotWriteSessionState::default();

        state.note_removed_payloads(SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD - 1);
        assert!(!state.request_compaction);

        state.note_removed_payloads(1);
        assert!(state.request_compaction);
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
            let _ = session.begin_group(BeginGroupInput::new(key, None));
        }

        state.group_stack[0].next_child_index = table.groups.len() + 1;

        assert_eq!(
            state.validate(&table),
            Err(SlotInvariantError::WriterFrameOutOfBounds {
                frame_index: 1,
                group_anchor: table.groups[0].anchor,
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
            let _ = session.begin_group(BeginGroupInput::new(root_key, None));

            let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
            let _ = session.begin_group(BeginGroupInput::new(child_key, None));

            let grandchild_key = session.preview_group_key(GroupKeySeed::unkeyed(12));
            let _ = session.begin_group(BeginGroupInput::new(grandchild_key, None));
        }

        let root_anchor = table.groups[0].anchor;
        let child_anchor = table.groups[1].anchor;
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
            let _ = session.begin_group(BeginGroupInput::new(key, None));
            let _ = session.value_slot(|| 17_i32);
            session.record_node(31, 1);
        }

        assert_eq!(state.group_stack[0].old_payload_len, 0);
        assert_eq!(state.group_stack[0].payload_cursor, 1);
        assert_eq!(state.group_stack[0].old_node_len, 0);
        assert_eq!(state.group_stack[0].node_cursor, 1);
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
            let _ = session.begin_group(BeginGroupInput::new(root_key, None));

            let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
            let child = session.begin_group(BeginGroupInput::new(child_key, None));
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
            let _ = session.begin_group(BeginGroupInput::new(key, None));
            session.record_node(31, 1);
            let _ = session.finish_group_body();
            session.end_group();
        }

        state.reset_for_pass(SlotPassMode::Compose);
        {
            let mut session = table.write_session(&mut lifecycle, &mut state);
            let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
            let _ = session.begin_group(BeginGroupInput::new(key, None));
            let _ = session.finish_group_body();
        }

        assert_eq!(state.group_stack[0].old_node_len, 1);
        assert_eq!(table.groups[0].node_len, 0);
        assert_eq!(state.validate(&table), Ok(()));
    }
}
