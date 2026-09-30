use super::*;
use crate::slot::{GroupKeySeed, SlotInvariantError, SlotLifecycleCoordinator, SlotTable};

fn host_frame_capacity(host: &crate::SlotsHost) -> (usize, usize) {
    let inner = host.inner.borrow();
    let state = &inner.pass.state;
    (state.group_stack.capacity(), state.frame_pool.capacity())
}

fn write_host_groups(host: &crate::SlotsHost) {
    host.with_write_session(|session| {
        for key in 1..=6 {
            let key = session.preview_group_key(GroupKeySeed::unkeyed(key));
            let _ = session.begin_group(key, None, None);
        }
        for _ in 1..=6 {
            let _ = session.finish_group_body();
            session.end_group();
        }
    });
}

#[test]
fn slots_host_reuses_frame_buffers_between_successful_passes() {
    let host = crate::SlotsHost::new(SlotTable::new());
    let mut applier = crate::MemoryApplier::new();
    host.begin_pass(SlotPassMode::Compose);
    write_host_groups(&host);
    let capacity = host_frame_capacity(&host);
    assert!(capacity.0 >= 6 && capacity.1 >= 6);
    host.finish_pass(&mut applier).expect("first pass");
    assert!(!host.has_active_pass());
    assert_eq!(host.try_push_branch_fold(1), None);
    assert!(!host.try_close_branch_fold(0));
    host.finish_pass(&mut applier).expect("idle finish");
    assert_eq!(host_frame_capacity(&host), capacity);

    for _ in 0..3 {
        host.begin_pass(SlotPassMode::Compose);
        assert_eq!(host_frame_capacity(&host), capacity, "reuse before writing");
        write_host_groups(&host);
        assert_eq!(host_frame_capacity(&host), capacity);
        host.finish_pass(&mut applier).expect("subsequent pass");
        assert_eq!(host.borrow().group_count(), 6, "pass keys start afresh");
    }
}

#[test]
fn slots_host_discards_abandoned_pass_buffers() {
    let host = crate::SlotsHost::new(SlotTable::new());
    host.begin_pass(SlotPassMode::Compose);
    write_host_groups(&host);
    assert_ne!(host_frame_capacity(&host), (0, 0));
    host.abandon_active_pass();
    assert!(!host.has_active_pass());
    assert_eq!(host_frame_capacity(&host), (0, 0));
    host.begin_pass(SlotPassMode::Recompose);
    assert_eq!(host_frame_capacity(&host), (0, 0));
    host.finish_pass(&mut crate::MemoryApplier::new())
        .expect("fresh pass");
}

#[test]
fn slots_host_reset_and_transfer_release_cached_pass_buffers() {
    for transfer in [false, true] {
        let host = crate::SlotsHost::new(SlotTable::new());
        host.begin_pass(SlotPassMode::Compose);
        write_host_groups(&host);
        host.finish_pass(&mut crate::MemoryApplier::new())
            .expect("pass");
        assert_ne!(host_frame_capacity(&host), (0, 0));
        if transfer {
            let table = host.take_table_for_transfer().expect("transfer");
            assert_eq!(table.group_count(), 6);
        } else {
            host.reset().expect("reset");
        }
        assert!(!host.has_active_pass());
        assert_eq!(host_frame_capacity(&host), (0, 0));
        assert_eq!(host.borrow().group_count(), 0);
    }
}

#[cfg(debug_assertions)]
#[test]
fn slots_host_discards_pass_buffers_after_validation_failure() {
    let host = crate::SlotsHost::new(SlotTable::new());
    host.begin_pass(SlotPassMode::Compose);
    host.with_write_session(|session| {
        let key = session.preview_group_key(GroupKeySeed::unkeyed(10));
        let _ = session.begin_group(key, None, None);
        session.state.group_stack[0].next_child_index = 100;
    });
    assert!(host.finish_pass(&mut crate::MemoryApplier::new()).is_err());
    assert!(!host.has_active_pass());
    assert_eq!(host_frame_capacity(&host), (0, 0));
    host.begin_pass(SlotPassMode::Recompose);
    host.finish_pass(&mut crate::MemoryApplier::new())
        .expect("recovered pass");
}

#[test]
fn slots_host_discards_cached_buffers_after_apply_failure() {
    let host = crate::SlotsHost::new(SlotTable::new());
    host.begin_pass(SlotPassMode::Compose);
    write_host_groups(&host);
    host.finish_pass(&mut crate::MemoryApplier::new())
        .expect("pass");
    assert_ne!(host_frame_capacity(&host), (0, 0));
    host.abandon_after_apply_failure();
    assert_eq!(host_frame_capacity(&host), (0, 0));
    assert_eq!(host.borrow().group_count(), 0);
}

#[test]
fn reused_slot_pass_does_not_repeat_a_compaction_request() {
    let host = crate::SlotsHost::new(SlotTable::new());
    let mut applier = crate::MemoryApplier::new();
    host.begin_pass(SlotPassMode::Compose);
    host.with_write_session(|session| {
        session
            .state
            .note_removed_payloads(SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD);
    });
    let first = host.finish_pass(&mut applier).expect("first pass");
    assert!(first.outcome.compacted && first.outcome.compact_payload_storage);
    host.begin_pass(SlotPassMode::Recompose);
    let second = host.finish_pass(&mut applier).expect("subsequent pass");
    assert!(!second.outcome.compacted && !second.outcome.compact_payload_storage);
}

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
