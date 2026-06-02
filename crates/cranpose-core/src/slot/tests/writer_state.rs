use super::*;
use crate::slot::DirectChildRange;

fn active_group_anchors(table: &SlotTable) -> Vec<AnchorId> {
    table.groups.iter().map(|group| group.anchor).collect()
}

#[test]
fn skip_group_advances_by_exact_subtree_size_and_keeps_nodes_stable() {
    const PARENT_KEY: Key = 500;
    const CHILD_A_KEY: Key = 501;
    const GRANDCHILD_KEY: Key = 502;
    const CHILD_B_KEY: Key = 503;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_A_KEY, None);
        session.record_node_with_parent(10, 1, None);
        begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.record_node_with_parent(20, 1, Some(10));
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_B_KEY, None);
        session.record_node_with_parent(30, 1, None);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let child_b_kind = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child_a = begin_unkeyed(session, CHILD_A_KEY, None);
        assert_eq!(child_a.kind, GroupStartKind::Reused);
        assert_eq!(node_ids_in_current_subtree(session), vec![10, 20]);
        session.skip_group();
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        assert_eq!(child_a_result.root_nodes, vec![10]);
        assert!(child_a_result.was_skipped);
        session.end_group();

        let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        child_b.kind
    });
    harness.finish_pass();

    assert_eq!(child_b_kind, GroupStartKind::Reused);
}

#[test]
fn scope_index_resolves_active_groups_and_omits_detached_ones() {
    const GROUP_KEY: Key = 600;
    const SCOPE_ID: ScopeId = 55;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(started.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("active scope must resolve through the scope index");
        assert_eq!(group.index(), 0);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();

    assert!(harness.table.groups.is_empty());
    assert!(
        !anchor_is_active(&harness.table, group_anchor),
        "disposed groups must invalidate their active anchor lookup"
    );
    assert_eq!(
        harness.table.anchors.state(group_anchor),
        None,
        "disposed groups must remove their anchor lookup"
    );

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        assert!(
            session.begin_recompose_at_scope(SCOPE_ID).is_none(),
            "detached or disposed scopes must not resolve through the active-table scope index"
        );
    });
    harness.finish_pass();
}

#[test]
fn scope_lookup_ignores_anchor_with_corrupt_active_group_index() {
    const GROUP_KEY: Key = 601;
    const SCOPE_ID: ScopeId = 66;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(started.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    harness.table.anchors.set_active(group_anchor, 99);

    assert_eq!(
        harness.table.active_group_for_scope(SCOPE_ID),
        None,
        "malformed anchor indexes must not abort scoped recomposition lookup"
    );
}

#[test]
fn active_group_index_rejects_corrupt_anchor_index() {
    const GROUP_KEY: Key = 602;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    harness.table.anchors.set_active(group_anchor, 99);

    assert_eq!(
        harness.table.active_group_index(group_anchor),
        None,
        "corrupt active anchor indexes must resolve as absent"
    );
    assert_eq!(
        harness.table.direct_child_range(group_anchor),
        DirectChildRange::new(1, 1),
        "child lookup under a corrupt parent anchor must produce an empty range"
    );
}

#[test]
fn skip_and_end_group_with_stale_frame_anchor_do_not_panic() {
    const GROUP_KEY: Key = 603;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);

    {
        let mut session = harness
            .table
            .write_session(&mut harness.lifecycle, &mut harness.state);
        let started = begin_unkeyed(&mut session, GROUP_KEY, None);
        session.table.anchors.mark_detached(started.anchor);

        session.skip_group();
        session.end_group();
    }

    assert!(
        harness.state.group_stack.is_empty(),
        "ending a stale group frame must still unwind the writer stack"
    );
}

#[test]
fn begin_group_repairs_corrupt_active_anchor_index_from_group_record() {
    const GROUP_KEY: Key = 604;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    harness.table.anchors.set_active(group_anchor, 99);

    harness.begin_pass(SlotPassMode::Compose);
    let reused = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(started.kind, GroupStartKind::Reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    assert_eq!(reused, group_anchor);
    assert_eq!(harness.table.active_group_index(group_anchor), Some(0));
}

#[test]
fn node_count_adjustment_stops_at_corrupt_parent_anchor() {
    const PARENT_KEY: Key = 606;
    const CHILD_KEY: Key = 607;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let child_anchor = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        child.anchor
    });
    harness.finish_pass();

    harness.table.groups[1].parent_anchor = AnchorId::new(99_999);

    let update = harness
        .table
        .record_node_at_cursor(child_anchor, 0, 42, None, 1);

    assert_eq!(
        update,
        NodeSlotUpdate::Inserted {
            id: 42,
            generation: 1
        }
    );
    assert_eq!(harness.table.groups[1].subtree_node_count, 1);
    assert_eq!(harness.table.groups[0].subtree_node_count, 0);
}

#[test]
fn group_span_adjustment_stops_at_corrupt_parent_anchor() {
    const PARENT_KEY: Key = 608;
    const CHILD_KEY: Key = 609;
    const GRANDCHILD_KEY: Key = 610;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let child_anchor = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        child.anchor
    });
    harness.finish_pass();

    harness.table.groups[1].parent_anchor = AnchorId::new(99_998);
    let cursor = ChildCursor::new(child_anchor, 2);

    let grandchild_anchor = harness
        .table
        .insert_new_group(cursor, GroupKey::new(GRANDCHILD_KEY, None, 0));

    assert!(grandchild_anchor.is_valid());
    assert_eq!(harness.table.groups.len(), 3);
    assert_eq!(harness.table.groups[1].subtree_len, 2);
    assert_eq!(harness.table.groups[0].subtree_len, 2);
}

#[test]
fn value_slot_with_stale_group_frame_uses_recovery_group() {
    const GROUP_KEY: Key = 605;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);

    let slot = {
        let mut session = harness
            .table
            .write_session(&mut harness.lifecycle, &mut harness.state);
        let started = begin_unkeyed(&mut session, GROUP_KEY, None);
        session.table.anchors.set_active(started.anchor, 99);
        session.value_slot_with_kind(PayloadKind::Internal, || 17_i32)
    };

    assert!(harness.state.group_stack.is_empty());
    assert_eq!(*harness.table.read_value::<i32>(slot), 17);
}

#[test]
fn end_group_repairs_corrupt_subtree_len_before_advancing_parent_cursor() {
    const PARENT_KEY: Key = 605_001;
    const CHILD_KEY: Key = 605_002;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);

    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.table.groups[child.group.index()].subtree_len = u32::MAX;

        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_frame = session
            .state
            .group_stack
            .last()
            .expect("parent frame should remain open");
        assert_eq!(parent_frame.next_child_index, 2);
        assert_eq!(
            session.table.groups[child.group.index()].subtree_len,
            1,
            "child subtree span should be repaired from active depth structure"
        );

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();
}

#[test]
fn skip_group_repairs_corrupt_subtree_len_before_advancing_frame_cursor() {
    const PARENT_KEY: Key = 605_003;
    const CHILD_KEY: Key = 605_004;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        session.table.groups[parent.group.index()].subtree_len = u32::MAX;

        session.skip_group();

        let parent_frame = session
            .state
            .group_stack
            .last()
            .expect("skipped parent frame should remain open");
        assert_eq!(parent_frame.next_child_index, 2);
        assert_eq!(
            session.table.groups[parent.group.index()].subtree_len,
            2,
            "parent subtree span should be repaired from active depth structure"
        );

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();
}

#[test]
fn detached_subtree_preserves_root_nodes_from_stored_parent_links() {
    const PARENT_KEY: Key = 611;
    const CHILD_KEY: Key = 612;
    const GRANDCHILD_KEY: Key = 613;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(41, 1, None);
        begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.record_node_with_parent(42, 1, Some(41));
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![41, 42]);
    let mut root_nodes = Vec::new();
    detached.collect_root_nodes_into(&mut root_nodes);
    assert_eq!(root_nodes, vec![41]);
}

#[test]
fn set_group_scope_replaces_previous_scope_lookup() {
    const GROUP_KEY: Key = 610;
    const OLD_SCOPE_ID: ScopeId = 56;
    const NEW_SCOPE_ID: ScopeId = 57;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.set_group_scope(started.group, OLD_SCOPE_ID);
        session.set_group_scope(started.group, NEW_SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        assert!(
            session.begin_recompose_at_scope(OLD_SCOPE_ID).is_none(),
            "replaced scope ids must leave no stale active lookup"
        );
        let group = session
            .begin_recompose_at_scope(NEW_SCOPE_ID)
            .expect("the latest scope id must resolve through the scope index");
        assert_eq!(group.index(), 0);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass();
}

#[test]
fn scope_index_keeps_scope_across_reuse_and_keyed_move() {
    const PARENT_KEY: Key = 620;
    const STATIC_KEY: Key = 621;
    const FIRST_KEY: Key = 622;
    const SCOPED_KEY: Key = 623;
    const SCOPE_ID: ScopeId = 624;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let scoped_anchor = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, FIRST_KEY, None);
        assert_eq!(first.kind, GroupStartKind::Inserted);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let scoped = begin_keyed(session, STATIC_KEY, SCOPED_KEY, None);
        assert_eq!(scoped.kind, GroupStartKind::Inserted);
        session.set_group_scope(scoped.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        scoped.anchor
    });
    harness.finish_pass();
    assert_eq!(
        harness.table.scope_index_anchor(SCOPE_ID),
        Some(scoped_anchor)
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let first = begin_keyed(session, STATIC_KEY, FIRST_KEY, None);
        assert_eq!(first.kind, GroupStartKind::Reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let scoped = begin_keyed(session, STATIC_KEY, SCOPED_KEY, None);
        assert_eq!(scoped.kind, GroupStartKind::Reused);
        assert_eq!(scoped.anchor, scoped_anchor);
        session.set_group_scope(scoped.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();
    assert_eq!(
        harness.table.scope_index_anchor(SCOPE_ID),
        Some(scoped_anchor)
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let scoped = begin_keyed(session, STATIC_KEY, SCOPED_KEY, None);
        assert_eq!(scoped.kind, GroupStartKind::Moved);
        assert_eq!(scoped.anchor, scoped_anchor);
        session.set_group_scope(scoped.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let first = begin_keyed(session, STATIC_KEY, FIRST_KEY, None);
        assert_eq!(first.kind, GroupStartKind::Reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(
        harness.table.scope_index_anchor(SCOPE_ID),
        Some(scoped_anchor)
    );
    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("moved scoped group must resolve through the active scope index");
        assert_eq!(session.table.active_group_anchor(group), scoped_anchor);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass();
}

#[test]
fn assigning_same_scope_to_different_active_group_is_rejected() {
    const FIRST_KEY: Key = 625;
    const SECOND_KEY: Key = 626;
    const SCOPE_ID: ScopeId = 627;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let first = begin_unkeyed(session, FIRST_KEY, None);
        session.set_group_scope(first.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let second = begin_unkeyed(session, SECOND_KEY, None);
        assert!(
            !session.set_group_scope(second.group, SCOPE_ID),
            "one scope id must not be assigned to two active groups"
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(
        harness.table.scope_index_anchor(SCOPE_ID),
        Some(harness.table.groups[0].anchor)
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn restoring_scope_that_conflicts_with_active_group_gets_fresh_scope_owner() {
    const PARENT_KEY: Key = 628;
    const DETACHED_CHILD_KEY: Key = 629;
    const ACTIVE_CHILD_KEY: Key = 630;
    const CONFLICT_SCOPE: ScopeId = 631;

    let (mut harness, detached, _) = detached_single_child_with_options(
        PARENT_KEY,
        DETACHED_CHILD_KEY,
        Some(CONFLICT_SCOPE),
        false,
        false,
    );
    let parent_anchor = harness.table.groups[0].anchor;

    harness.begin_pass(SlotPassMode::Compose);
    let active_conflict_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let active = begin_unkeyed(session, ACTIVE_CHILD_KEY, None);
        session.set_group_scope(active.group, CONFLICT_SCOPE);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        active.anchor
    });
    harness.finish_pass();
    assert_eq!(
        harness.table.scope_index_anchor(CONFLICT_SCOPE),
        Some(active_conflict_anchor)
    );

    let before = active_group_anchors(&harness.table);
    let restore_key = detached.root_key();
    let insert_index = harness.table.direct_child_range(parent_anchor).end();
    let restored_anchor = restore_detached_child(
        &mut harness.table,
        parent_anchor,
        insert_index,
        restore_key,
        detached,
    );

    let after = active_group_anchors(&harness.table);
    assert_eq!(after[..before.len()], before);
    assert!(after.contains(&restored_anchor));
    assert_eq!(
        harness.table.scope_index_anchor(CONFLICT_SCOPE),
        Some(active_conflict_anchor)
    );
    let restored_index = harness.table.current_group_index(restored_anchor);
    assert_eq!(harness.table.groups[restored_index].scope_id, None);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn released_anchors_are_reused_without_sparse_growth() {
    const GROUP_COUNT: usize = 2048;

    let mut harness = SlotHarness::new();
    let first_anchor = harness.session(|session| {
        let mut first_anchor = None;
        for key in 0..GROUP_COUNT {
            let started = begin_unkeyed(session, key as Key + 10_000, None);
            first_anchor.get_or_insert(started.anchor);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        first_anchor.expect("first anchor")
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();
    assert_eq!(harness.table.anchor_state(first_anchor), None);
    let stats_after_clear = harness.table.debug_stats();
    let capacity_after_clear = stats_after_clear.anchor_capacity;
    assert!(
        capacity_after_clear <= GROUP_COUNT * 2,
        "releasing groups must not leave sparse anchor storage behind: after_clear={capacity_after_clear}",
    );
    assert_eq!(stats_after_clear.active_anchor_count, 0);
    assert_eq!(stats_after_clear.anchor_slot_count, 0);
    assert_eq!(stats_after_clear.anchor_sparse_count, 0);
    assert_eq!(stats_after_clear.detached_anchor_count, 0);
    assert_eq!(stats_after_clear.invalidated_anchor_count, 0);
    assert_eq!(stats_after_clear.free_anchor_count, GROUP_COUNT);

    harness.begin_pass(SlotPassMode::Compose);
    let reused_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, 90_000, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    let stats_after_reuse = harness.table.debug_stats();
    let capacity_after_reuse = stats_after_reuse.anchor_capacity;
    assert!(reused_anchor.generation > 1);
    assert_ne!(reused_anchor, first_anchor);
    assert!(
        capacity_after_reuse <= GROUP_COUNT * 2,
        "reusing released anchors must keep storage bounded: after_reuse={capacity_after_reuse}",
    );
    assert_eq!(stats_after_reuse.active_anchor_count, 1);
    assert_eq!(stats_after_reuse.anchor_slot_count, 1);
    assert_eq!(stats_after_reuse.anchor_sparse_count, 0);
    assert_eq!(stats_after_reuse.detached_anchor_count, 0);
    assert_eq!(stats_after_reuse.invalidated_anchor_count, 0);
    assert_eq!(stats_after_reuse.free_anchor_count, GROUP_COUNT - 1);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn stale_group_handle_does_not_alias_recreated_group() {
    const OLD_KEY: Key = 611;
    const NEW_KEY: Key = 612;
    const SCOPE_ID: ScopeId = 58;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let stale_group = harness.session(|session| {
        let started = begin_unkeyed(session, OLD_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.group
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();
    assert!(harness.table.groups.is_empty());

    harness.begin_pass(SlotPassMode::Compose);
    let recreated_group = harness.session(|session| {
        let started = begin_unkeyed(session, NEW_KEY, None);
        assert_eq!(started.group.index(), stale_group.index());
        assert_ne!(started.group.generation(), stale_group.generation());

        assert!(
            !session.set_group_scope(stale_group, SCOPE_ID),
            "stale group handles must not alias a newly created group"
        );

        session.set_group_scope(started.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.group
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("the recreated group must resolve through the scope index");
        assert_eq!(group, recreated_group);
        assert_ne!(group.generation(), stale_group.generation());
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass();
}

#[test]
fn moved_group_invalidates_previous_active_group_id() {
    const STATIC_KEY: Key = 613;
    const FIRST_KEY: Key = 614;
    const SECOND_KEY: Key = 615;
    const SCOPE_ID: ScopeId = 616;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_anchor, second_anchor, stale_second_group) = harness.session(|session| {
        let first = begin_keyed(session, STATIC_KEY, FIRST_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let second = begin_keyed(session, STATIC_KEY, SECOND_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        (first.anchor, second.anchor, second.group)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let moved_second = begin_keyed(session, STATIC_KEY, SECOND_KEY, None);
        assert_eq!(moved_second.kind, GroupStartKind::Moved);
        assert_eq!(moved_second.anchor, second_anchor);
        assert_ne!(moved_second.group, stale_second_group);

        assert!(
            !session.set_group_scope(stale_second_group, SCOPE_ID),
            "moved active group ids must not alias the group shifted into the previous index"
        );

        session.set_group_scope(moved_second.group, SCOPE_ID);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let shifted_first = begin_keyed(session, STATIC_KEY, FIRST_KEY, None);
        assert_eq!(shifted_first.kind, GroupStartKind::Reused);
        assert_eq!(shifted_first.anchor, first_anchor);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Recompose);
    let resolved = harness.session(|session| {
        let group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("stable scope lookup must resolve through the moved anchor");
        assert_ne!(group, stale_second_group);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
        group
    });
    harness.finish_pass();
    assert_eq!(harness.table.active_group_anchor(resolved), second_anchor);
}
