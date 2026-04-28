use super::*;

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

        let stale_assign = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.set_group_scope(stale_group, SCOPE_ID);
        }));
        assert!(
            stale_assign.is_err(),
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

        let stale_assign = panic::catch_unwind(AssertUnwindSafe(|| {
            session.set_group_scope(stale_second_group, SCOPE_ID);
        }));
        assert!(
            stale_assign.is_err(),
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
