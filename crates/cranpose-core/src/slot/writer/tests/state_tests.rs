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
        let _ = session.begin_group(key, None, None);
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
        let _ = session.begin_group(root_key, None, None);

        let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
        let _ = session.begin_group(child_key, None, None);

        let grandchild_key = session.preview_group_key(GroupKeySeed::unkeyed(12));
        let _ = session.begin_group(grandchild_key, None, None);
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
        let _ = session.begin_group(key, None, None);
        let _ = session.value_slot_with_kind(
            crate::slot::PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || 17_i32,
        );
        session.record_node_with_parent(31, 1, None, crate::slot::BRANCH_PATH_ROOT);
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
        let _ = session.begin_group(root_key, None, None);

        let child_key = session.preview_group_key(GroupKeySeed::unkeyed(11));
        let child = session.begin_group(child_key, None, None);
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
        let _ = session.begin_group(key, None, None);
        session.record_node_with_parent(31, 1, None, crate::slot::BRANCH_PATH_ROOT);
        let _ = session.finish_group_body();
        session.end_group();
    }

    state.reset_for_pass(SlotPassMode::Compose);
    {
        let mut session = table.write_session(&mut lifecycle, &mut state);
        let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
        let _ = session.begin_group(key, None, None);
        let _ = session.finish_group_body();
    }

    assert_eq!(state.group_stack[0].old_node_len, 1);
    assert_eq!(table.group_node_len_at(0), 0);
    state.flush_payload_location_refreshes(&mut table);
    assert_eq!(state.validate(&table), Ok(()));
}
