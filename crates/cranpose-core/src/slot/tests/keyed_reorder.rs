use super::*;

#[test]
fn keyed_sibling_reorder_preserves_values_and_anchors() {
    const PARENT_KEY: Key = 400;
    const STATIC_KEY: Key = 401;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, first_anchor, second_slot, second_anchor) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (first_slot, first.anchor, second_slot, second.anchor)
    });
    harness.finish_pass();
    harness.table.write_value(first_slot, 101_i32);
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let (
        second_kind,
        reordered_second_anchor,
        reordered_second_slot,
        first_kind,
        reordered_first_anchor,
        reordered_first_slot,
    ) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (
            second.kind,
            second.anchor,
            second_slot,
            first.kind,
            first.anchor,
            first_slot,
        )
    });
    harness.finish_pass();

    assert_eq!(second_kind, GroupStartKind::Moved);
    assert_eq!(reordered_second_anchor, second_anchor);
    assert_eq!(first_kind, GroupStartKind::Reused);
    assert_eq!(reordered_first_anchor, first_anchor);
    assert_eq!(*harness.table.read_value::<i32>(reordered_second_slot), 202);
    assert_eq!(*harness.table.read_value::<i32>(reordered_first_slot), 101);
}

#[test]
fn debug_stats_report_subtree_move_work_spans() {
    const PARENT_KEY: Key = 402;
    const STATIC_KEY: Key = 403;
    const NESTED_KEY: Key = 404;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_keyed(session, STATIC_KEY, 1, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        session.record_node_with_parent(11, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        begin_keyed(session, STATIC_KEY, 2, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        session.record_node_with_parent(22, 1, None);

        begin_unkeyed(session, NESTED_KEY, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 30_i32);
        session.record_node_with_parent(33, 1, None);
        let nested_result = session.finish_group_body();
        assert!(nested_result.detached_children.is_empty());
        session.end_group();

        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    let moved_kind = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let moved = begin_keyed(session, STATIC_KEY, 2, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        session.record_node_with_parent(22, 1, None);

        begin_unkeyed(session, NESTED_KEY, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        session.record_node_with_parent(33, 1, None);
        let nested_result = session.finish_group_body();
        assert!(nested_result.detached_children.is_empty());
        session.end_group();

        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        begin_keyed(session, STATIC_KEY, 1, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        session.record_node_with_parent(11, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        moved.kind
    });
    harness.finish_pass();

    assert_eq!(moved_kind, GroupStartKind::Moved);

    let after = harness.table.debug_stats().mutation;
    assert_eq!(after.subtree_move_count - before.subtree_move_count, 1);
    assert_eq!(after.moved_group_count - before.moved_group_count, 2);
    assert_eq!(after.moved_group_max_span, 2);
    assert_eq!(after.moved_payload_count - before.moved_payload_count, 2);
    assert_eq!(after.moved_payload_max_span, 2);
    assert_eq!(after.moved_node_count - before.moved_node_count, 2);
    assert_eq!(after.moved_node_max_span, 2);
    assert_eq!(
        after.payload_anchor_index_rebuild_count - before.payload_anchor_index_rebuild_count,
        0
    );
    assert_eq!(
        after.payload_anchor_index_rebuild_group_count
            - before.payload_anchor_index_rebuild_group_count,
        0
    );
    assert_eq!(
        after.payload_anchor_index_rebuild_group_max_span,
        before.payload_anchor_index_rebuild_group_max_span
    );
    assert_eq!(
        after.payload_anchor_index_rebuild_payload_count
            - before.payload_anchor_index_rebuild_payload_count,
        0
    );
    assert_eq!(
        after.payload_anchor_index_rebuild_payload_max_span,
        before.payload_anchor_index_rebuild_payload_max_span
    );
    assert_eq!(
        after.group_index_refresh_count - before.group_index_refresh_count,
        1
    );
    assert_eq!(
        after.group_index_refresh_group_count - before.group_index_refresh_group_count,
        3
    );
    assert_eq!(after.group_index_refresh_max_span, 3);
    assert!(
        after.segment_range_update_count > before.segment_range_update_count,
        "subtree moves with payloads and nodes must report segment range maintenance"
    );
    assert!(
        after.segment_range_update_group_count > before.segment_range_update_group_count,
        "segment range maintenance must report affected group span"
    );
}

#[test]
fn detaching_unvisited_siblings_batches_group_index_refresh() {
    const ROOT_KEY: Key = 4041;
    const PARENT_KEY: Key = 4042;
    const CHILD_KEY: Key = 4043;
    const TRAILING_KEY: Key = 4044;
    const CHILD_COUNT: Key = 3;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let trailing_anchor = harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);

        begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in 0..CHILD_COUNT {
            begin_keyed(session, CHILD_KEY, explicit_key, None);
            let child_result = session.finish_group_body();
            assert!(child_result.detached_children.is_empty());
            session.end_group();
        }
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        let trailing = begin_unkeyed(session, TRAILING_KEY, None);
        let trailing_result = session.finish_group_body();
        assert!(trailing_result.detached_children.is_empty());
        session.end_group();

        let root_result = session.finish_group_body();
        assert!(root_result.detached_children.is_empty());
        session.end_group();

        trailing.anchor
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    let (detached_children, trailing_kind, refreshed_trailing_anchor) =
        harness.session(|session| {
            begin_unkeyed(session, ROOT_KEY, None);

            begin_unkeyed(session, PARENT_KEY, None);
            let parent_result = session.finish_group_body();
            session.end_group();

            let trailing = begin_unkeyed(session, TRAILING_KEY, None);
            let trailing_result = session.finish_group_body();
            assert!(trailing_result.detached_children.is_empty());
            session.end_group();

            let root_result = session.finish_group_body();
            assert!(root_result.detached_children.is_empty());
            session.end_group();

            (
                parent_result.detached_children,
                trailing.kind,
                trailing.anchor,
            )
        });
    harness.finish_pass();
    assert_eq!(detached_children.len(), CHILD_COUNT as usize);
    for subtree in detached_children {
        harness.table.invalidate_detached_subtree_anchors(&subtree);
        harness.lifecycle.queue_subtree_disposal(subtree);
    }
    harness.lifecycle.flush_pending_drops();

    let after = harness.table.debug_stats().mutation;
    assert_eq!(trailing_kind, GroupStartKind::Reused);
    assert_eq!(refreshed_trailing_anchor, trailing_anchor);
    assert_eq!(harness.table.current_group_index(trailing_anchor), 2);
    assert_eq!(
        after.group_index_refresh_count - before.group_index_refresh_count,
        1
    );
    assert_eq!(
        after.group_index_refresh_group_count - before.group_index_refresh_group_count,
        1
    );
}

#[test]
fn debug_stats_report_payload_anchor_index_rebuild_work_spans() {
    const GROUP_KEY: Key = 4051;

    let mut table = composed_group_with_value_and_node_table(GROUP_KEY);
    let before = table.debug_stats().mutation;

    table.rebuild_payload_anchor_index_for_group_range(GroupRange::new(
        0,
        table.debug_stats().group_count,
    ));

    let after = table.debug_stats().mutation;
    assert_eq!(
        after.payload_anchor_index_rebuild_count - before.payload_anchor_index_rebuild_count,
        1
    );
    assert_eq!(
        after.payload_anchor_index_rebuild_group_count
            - before.payload_anchor_index_rebuild_group_count,
        1
    );
    assert_eq!(after.payload_anchor_index_rebuild_group_max_span, 1);
    assert_eq!(
        after.payload_anchor_index_rebuild_payload_count
            - before.payload_anchor_index_rebuild_payload_count,
        1
    );
    assert_eq!(after.payload_anchor_index_rebuild_payload_max_span, 1);
}

#[test]
fn debug_stats_report_scope_index_rebuild_work_spans() {
    const GROUP_KEY: Key = 4052;
    const SCOPE_ID: ScopeId = 405_200;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let group = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(group.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;
    harness.table.recompute_scope_index();
    let after = harness.table.debug_stats().mutation;

    assert_eq!(
        after.scope_index_rebuild_count - before.scope_index_rebuild_count,
        1
    );
    assert_eq!(
        after.scope_index_rebuild_scope_count - before.scope_index_rebuild_scope_count,
        1
    );
    assert_eq!(after.scope_index_rebuild_max_span, 1);
}

#[test]
fn large_keyed_sibling_reorder_preserves_values_and_anchors() {
    const PARENT_KEY: Key = 405;
    const STATIC_KEY: Key = 406;
    const CHILD_COUNT: Key = 24;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let original = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let mut children = Vec::new();

        for key in 1..=CHILD_COUNT {
            let child = begin_keyed(session, STATIC_KEY, key, None);
            let slot = session.value_slot_with_kind(PayloadKind::Internal, || key as i32);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
            children.push((key, child.anchor, slot));
        }

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        children
    });
    harness.finish_pass();

    for (key, _, slot) in original.iter().copied() {
        harness.table.write_value(slot, 10_000 + key as i32);
    }

    harness.begin_pass(SlotPassMode::Compose);
    let reordered = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let mut children = Vec::new();

        for key in (1..=CHILD_COUNT).rev() {
            let child = begin_keyed(session, STATIC_KEY, key, None);
            assert!(
                matches!(child.kind, GroupStartKind::Reused | GroupStartKind::Moved),
                "large keyed reorder must reuse or move existing child {key}, got {:?}",
                child.kind,
            );
            let slot = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
            children.push((key, child.anchor, slot));
        }

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        children
    });
    harness.finish_pass();

    for (key, anchor, slot) in reordered {
        let (_, original_anchor, _) = original
            .iter()
            .copied()
            .find(|(original_key, _, _)| *original_key == key)
            .expect("original child key must exist");
        assert_eq!(anchor, original_anchor);
        assert_eq!(*harness.table.read_value::<i32>(slot), 10_000 + key as i32);
    }
}

#[test]
fn keyed_sibling_search_never_matches_a_grandchild() {
    const PARENT_KEY: Key = 410;
    const STATIC_KEY: Key = 411;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (direct_anchor, grandchild_anchor) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let _first = begin_keyed(session, STATIC_KEY, 1, None);
        let grandchild = begin_keyed(session, STATIC_KEY, 2, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (second.anchor, grandchild.anchor)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let (moved_anchor, reused_grandchild_anchor) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let moved = begin_keyed(session, STATIC_KEY, 2, None);
        assert_eq!(moved.kind, GroupStartKind::Moved);
        let moved_result = session.finish_group_body();
        assert!(moved_result.detached_children.is_empty());
        session.end_group();

        let _first = begin_keyed(session, STATIC_KEY, 1, None);
        let reused_grandchild = begin_keyed(session, STATIC_KEY, 2, None);
        assert_eq!(reused_grandchild.kind, GroupStartKind::Reused);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (moved.anchor, reused_grandchild.anchor)
    });
    harness.finish_pass();

    assert_eq!(moved_anchor, direct_anchor);
    assert_eq!(reused_grandchild_anchor, grandchild_anchor);
}

#[test]
fn restored_keyed_sibling_can_move_on_a_later_pass() {
    const PARENT_KEY: Key = 440;
    const STATIC_KEY: Key = 441;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (second_slot, second_anchor) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Inserted);
        assert_eq!(third.kind, GroupStartKind::Inserted);
        (second_slot, second.anchor)
    });
    harness.finish_pass();
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Reused);
        assert_eq!(third.kind, GroupStartKind::Moved);
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let (restored_kind, restored_anchor, restored_slot) = harness.session(move |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, 2, Some(detached));
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Reused);
        assert_eq!(third.kind, GroupStartKind::Reused);
        (second.kind, second.anchor, second_slot)
    });
    harness.finish_pass();

    assert_eq!(restored_kind, GroupStartKind::Restored);
    assert_eq!(restored_anchor, second_anchor);
    assert_eq!(restored_slot, second_slot);
    assert_eq!(*harness.table.read_value::<i32>(restored_slot), 202);

    harness.begin_pass(SlotPassMode::Compose);
    let (moved_kind, moved_anchor, moved_slot) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let second = begin_keyed(session, STATIC_KEY, 2, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let first = begin_keyed(session, STATIC_KEY, 1, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let third = begin_keyed(session, STATIC_KEY, 3, None);
        let third_result = session.finish_group_body();
        assert!(third_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        assert_eq!(first.kind, GroupStartKind::Reused);
        assert_eq!(third.kind, GroupStartKind::Reused);
        (second.kind, second.anchor, second_slot)
    });
    harness.finish_pass();

    assert_eq!(moved_kind, GroupStartKind::Moved);
    assert_eq!(moved_anchor, second_anchor);
    assert_eq!(moved_slot, second_slot);
    assert_eq!(*harness.table.read_value::<i32>(moved_slot), 202);
}

#[test]
fn unkeyed_siblings_follow_positional_identity() {
    const PARENT_KEY: Key = 450;
    const SHARED_KEY: Key = 451;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, second_slot) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, SHARED_KEY, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, SHARED_KEY, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (first_slot, second_slot)
    });
    harness.finish_pass();
    harness.table.write_value(first_slot, 101_i32);
    harness.table.write_value(second_slot, 202_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let remaining_slot = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let remaining = begin_unkeyed(session, SHARED_KEY, None);
        assert_eq!(remaining.kind, GroupStartKind::Reused);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let result = session.finish_group_body();
        assert_eq!(result.detached_children.len(), 0);
        session.end_group();

        let parent_result = session.finish_group_body();
        assert_eq!(parent_result.detached_children.len(), 1);
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(
            *harness.table.read_value::<i32>(remaining_slot),
            101,
            "unkeyed duplicate siblings must follow positional identity, not the removed sibling's previous state",
        );
}

#[test]
fn duplicate_explicit_keys_are_rejected_during_writer_traversal() {
    const PARENT_KEY: Key = 460;
    const STATIC_KEY: Key = 461;
    const EXPLICIT_KEY: Key = 77;

    let mut table = SlotTable::new();
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let mut state = SlotWriteSessionState::default();
    state.reset_for_pass(SlotPassMode::Compose);
    {
        let mut session = table.write_session(&mut lifecycle, &mut state);
        begin_unkeyed(&mut session, PARENT_KEY, None);

        begin_keyed(&mut session, STATIC_KEY, EXPLICIT_KEY, None);
        let first = session.finish_group_body();
        assert!(first.detached_children.is_empty());
        session.end_group();

        let duplicate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            begin_keyed(&mut session, STATIC_KEY, EXPLICIT_KEY, None);
        }));
        assert!(
            duplicate.is_err(),
            "duplicate explicit sibling keys must fail before mutating the slot table",
        );

        let parent = session.finish_group_body();
        assert!(parent.detached_children.is_empty());
        session.end_group();
    }

    assert_eq!(table.validate(), Ok(()));
}
