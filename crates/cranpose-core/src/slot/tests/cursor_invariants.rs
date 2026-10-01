use super::*;

fn active_group_anchors(table: &SlotTable) -> Vec<AnchorId> {
    table.groups.iter().map(|group| group.anchor).collect()
}

fn composed_parent_with_two_children(
    parent_key: Key,
    first_child_key: Key,
    second_child_key: Key,
) -> (SlotHarness, AnchorId, AnchorId, AnchorId) {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, first_anchor, second_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, parent_key, None);

        let first = begin_unkeyed(session, first_child_key, None);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        let second = begin_unkeyed(session, second_child_key, None);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (parent.anchor, first.anchor, second.anchor)
    });
    harness.finish_pass();
    (harness, parent_anchor, first_anchor, second_anchor)
}

#[test]
fn begin_group_recovers_malformed_root_child_cursor_without_panicking() {
    const FIRST_KEY: Key = 10_020;
    const RECOVERY_KEY: Key = 10_021;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let first_anchor = harness.session(|session| {
        let first = begin_unkeyed(session, FIRST_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        first.anchor
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.state.root.next_child_index = usize::MAX;
    let recovered_anchor = harness.session(|session| {
        let recovered = begin_unkeyed(session, RECOVERY_KEY, None);
        assert_eq!(recovered.kind, GroupStartKind::Inserted);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        recovered.anchor
    });
    harness.finish_pass();

    assert_ne!(recovered_anchor, AnchorId::INVALID);
    assert!(anchor_is_active(&harness.table, recovered_anchor));
    assert!(!anchor_is_active(&harness.table, first_anchor));
    harness.table.debug_verify();
}

#[test]
fn keyed_sibling_move_later_child_to_earlier_cursor_succeeds() {
    const PARENT_KEY: Key = 10_031;
    const FIRST_CHILD_KEY: Key = 10_032;
    const SECOND_CHILD_KEY: Key = 10_033;

    let (mut harness, parent_anchor, first_anchor, second_anchor) =
        composed_parent_with_two_children(PARENT_KEY, FIRST_CHILD_KEY, SECOND_CHILD_KEY);

    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(first_anchor),
    );
    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(second_anchor), cursor);

    assert_eq!(
        active_group_anchors(&harness.table),
        vec![parent_anchor, second_anchor, first_anchor]
    );
    assert_eq!(
        harness.table.group_anchor_state(second_anchor),
        Some(AnchorState::Active(1))
    );
    assert_eq!(
        harness.table.group_anchor_state(first_anchor),
        Some(AnchorState::Active(2))
    );
    harness.table.debug_verify();
}

#[test]
fn keyed_sibling_move_same_cursor_is_no_op() {
    const PARENT_KEY: Key = 10_041;
    const FIRST_CHILD_KEY: Key = 10_042;
    const SECOND_CHILD_KEY: Key = 10_043;

    let (mut harness, parent_anchor, first_anchor, _) =
        composed_parent_with_two_children(PARENT_KEY, FIRST_CHILD_KEY, SECOND_CHILD_KEY);
    let before = active_group_anchors(&harness.table);

    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(first_anchor),
    );
    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(first_anchor), cursor);

    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn keyed_sibling_move_with_stale_root_anchor_is_no_op() {
    const PARENT_KEY: Key = 10_044;
    const FIRST_CHILD_KEY: Key = 10_045;
    const SECOND_CHILD_KEY: Key = 10_046;

    let (mut harness, parent_anchor, first_anchor, second_anchor) =
        composed_parent_with_two_children(PARENT_KEY, FIRST_CHILD_KEY, SECOND_CHILD_KEY);
    let before = active_group_anchors(&harness.table);

    harness.table.anchors.set_active(second_anchor, 99);
    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(first_anchor),
    );

    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(second_anchor), cursor);

    assert_eq!(active_group_anchors(&harness.table), before);
}

#[test]
fn insert_new_group_rejects_stale_parent_cursor_without_mutating_table() {
    const PARENT_KEY: Key = 10_047;
    const FIRST_CHILD_KEY: Key = 10_048;
    const SECOND_CHILD_KEY: Key = 10_049;
    const INSERTED_KEY: Key = 10_050;

    let (mut harness, _, _, _) =
        composed_parent_with_two_children(PARENT_KEY, FIRST_CHILD_KEY, SECOND_CHILD_KEY);
    let before = active_group_anchors(&harness.table);
    let cursor = ChildCursor::new(AnchorId::new(99_997), 0);

    let inserted = harness
        .table
        .insert_new_group(cursor, GroupKey::new(INSERTED_KEY, None, 0));

    assert_eq!(inserted, AnchorId::INVALID);
    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn insert_new_group_rejects_cursor_inside_existing_child_subtree() {
    const PARENT_KEY: Key = 10_060;
    const CHILD_KEY: Key = 10_061;
    const GRANDCHILD_KEY: Key = 10_062;
    const INSERTED_KEY: Key = 10_063;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, grandchild_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        begin_unkeyed(session, CHILD_KEY, None);
        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        (parent.anchor, grandchild.anchor)
    });
    harness.finish_pass();

    let before = active_group_anchors(&harness.table);
    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(grandchild_anchor),
    );

    let inserted = harness
        .table
        .insert_new_group(cursor, GroupKey::new(INSERTED_KEY, None, 0));

    assert_eq!(inserted, AnchorId::INVALID);
    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn keyed_sibling_move_ignores_earlier_sibling_to_later_cursor() {
    const PARENT_KEY: Key = 10_051;
    const FIRST_CHILD_KEY: Key = 10_052;
    const SECOND_CHILD_KEY: Key = 10_053;

    let (mut harness, parent_anchor, first_anchor, second_anchor) =
        composed_parent_with_two_children(PARENT_KEY, FIRST_CHILD_KEY, SECOND_CHILD_KEY);
    let before = active_group_anchors(&harness.table);
    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(second_anchor),
    );

    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(first_anchor), cursor);

    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn child_cursor_rejects_cross_parent_active_move() {
    const PARENT_A_KEY: Key = 10_001;
    const CHILD_A_KEY: Key = 10_002;
    const PARENT_B_KEY: Key = 10_003;
    const CHILD_B_KEY: Key = 10_004;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_a_anchor, child_b_anchor) = harness.session(|session| {
        let parent_a = begin_unkeyed(session, PARENT_A_KEY, None);
        begin_unkeyed(session, CHILD_A_KEY, None);
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();
        let parent_a_result = session.finish_group_body();
        assert!(parent_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, PARENT_B_KEY, None);
        let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();
        let parent_b_result = session.finish_group_body();
        assert!(parent_b_result.detached_children.is_empty());
        session.end_group();

        (parent_a.anchor, child_b.anchor)
    });
    harness.finish_pass();

    let before = active_group_anchors(&harness.table);
    let cursor = ChildCursor::new(
        parent_a_anchor,
        harness.table.direct_child_range(parent_a_anchor).end(),
    );
    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(child_b_anchor), cursor);

    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn child_cursor_rejects_grandchild_as_direct_sibling_move() {
    const PARENT_KEY: Key = 10_011;
    const CHILD_KEY: Key = 10_012;
    const GRANDCHILD_KEY: Key = 10_013;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_anchor, grandchild_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, None);
        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        (parent.anchor, child.anchor, grandchild.anchor)
    });
    harness.finish_pass();

    let before = active_group_anchors(&harness.table);
    let cursor = ChildCursor::new(
        parent_anchor,
        harness.table.current_group_index(child_anchor),
    );
    harness
        .table
        .move_later_sibling_subtree_to_cursor(ActiveSubtreeRoot::new(grandchild_anchor), cursor);

    assert_eq!(active_group_anchors(&harness.table), before);
    harness.table.debug_verify();
}

#[test]
fn restore_rejects_cursor_inside_existing_child_subtree() {
    const PARENT_KEY: Key = 10_021;
    const CHILD_KEY: Key = 10_022;
    const GRANDCHILD_KEY: Key = 10_023;
    const DETACHED_KEY: Key = 10_024;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, grandchild_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        begin_unkeyed(session, CHILD_KEY, None);
        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        begin_unkeyed(session, DETACHED_KEY, None);
        let detached_result = session.finish_group_body();
        assert!(detached_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        (parent.anchor, grandchild.anchor)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        begin_unkeyed(session, CHILD_KEY, None);
        begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    let detached_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let group_count_before = harness.table.groups.len();
    let grandchild_index = harness.table.current_group_index(grandchild_anchor);
    let restore_result = panic::catch_unwind(AssertUnwindSafe(|| {
        restore_detached_child(
            &mut harness.table,
            parent_anchor,
            grandchild_index,
            detached.root_key(),
            detached,
        );
    }));

    assert!(
        restore_result.is_err(),
        "restore must reject cursors that point inside an existing child subtree",
    );
    assert_eq!(harness.table.groups.len(), group_count_before);
    assert_eq!(
        harness.table.anchor_state(detached_anchor),
        Some(AnchorState::Detached),
    );
    harness.table.debug_verify();
}

#[test]
fn restore_rejects_group_anchor_that_is_not_detached() {
    const PARENT_KEY: Key = 10_031;
    const CHILD_KEY: Key = 10_032;

    let (mut harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let parent_anchor = harness.table.groups[0].anchor;
    let detached_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let group_count_before = harness.table.groups.len();

    harness.table.anchors.set_active(detached_anchor, 0);
    let restore_result = panic::catch_unwind(AssertUnwindSafe(|| {
        restore_detached_child(
            &mut harness.table,
            parent_anchor,
            1,
            detached.root_key(),
            detached,
        );
    }));
    harness.table.anchors.mark_detached(detached_anchor);

    assert!(
        restore_result.is_err(),
        "restore must reject group anchors that are already active",
    );
    assert_eq!(harness.table.groups.len(), group_count_before);
    assert_eq!(
        harness.table.anchor_state(detached_anchor),
        Some(AnchorState::Detached),
    );
    harness.table.debug_verify();
}

#[test]
fn restore_rejects_payload_anchor_that_is_not_detached() {
    const PARENT_KEY: Key = 10_041;
    const CHILD_KEY: Key = 10_042;

    let (mut harness, detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, true, false);
    let parent_anchor = harness.table.groups[0].anchor;
    let payload_anchor = detached
        .payload_anchors()
        .next()
        .expect("detached subtree must contain a payload anchor");
    let group_count_before = harness.table.groups.len();

    harness
        .table
        .payload_anchors
        .set_active(payload_anchor, parent_anchor, 0);
    let restore_result = panic::catch_unwind(AssertUnwindSafe(|| {
        restore_detached_child(
            &mut harness.table,
            parent_anchor,
            1,
            detached.root_key(),
            detached,
        );
    }));
    harness.table.payload_anchors.mark_detached(payload_anchor);

    assert!(
        restore_result.is_err(),
        "restore must reject payload anchors that are already active",
    );
    assert_eq!(harness.table.groups.len(), group_count_before);
    assert_eq!(
        harness.table.payload_anchor_lifecycle(payload_anchor),
        Some(PayloadAnchorLifecycle::Detached),
    );
    harness.table.debug_verify();
}
