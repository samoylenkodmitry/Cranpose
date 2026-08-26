use super::*;

#[test]
fn record_node_without_active_group_reports_insert_without_recording() {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);

    let recorded = harness.session(|session| {
        let recorded = session.record_node_with_parent(11, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(session.current_node_record(), None);
        recorded
    });
    harness.finish_pass();

    assert_eq!(
        recorded,
        NodeSlotUpdate::Inserted {
            id: 11,
            generation: 1,
        },
    );
    assert_eq!(harness.table.total_node_count(), 0);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn node_operations_with_stale_owner_anchor_do_not_panic_or_record() {
    const GROUP_KEY: Key = 364;

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

    harness.table.anchors.mark_detached(group_anchor);

    let update = harness.table.record_node_at_cursor(
        group_anchor,
        0,
        42,
        None,
        1,
        crate::slot::BRANCH_PATH_ROOT,
    );
    assert_eq!(
        update,
        NodeSlotUpdate::Inserted {
            id: 42,
            generation: 1
        }
    );
    assert_eq!(harness.table.node_identity_at_cursor(group_anchor, 0), None);
    assert!(harness
        .table
        .remove_group_node_tail_at_cursor(group_anchor, 0)
        .is_empty());
    assert_eq!(harness.table.total_node_count(), 0);
}

#[test]
fn node_tail_cleanup_repairs_corrupt_group_node_len() {
    const GROUP_KEY: Key = 364_002;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        session.record_node_with_parent(42, 1, None, crate::slot::BRANCH_PATH_ROOT);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.table.groups[0].node_len = 2;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let update = session.record_node_with_parent(42, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            update,
            NodeSlotUpdate::Reused {
                id: 42,
                generation: 1
            }
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.groups[0].node_len, 1);
    assert_eq!(harness.table.total_node_count(), 1);
    assert_eq!(harness.table.group_node_record_at(0, 0).id, 42);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn node_insertion_repairs_corrupt_subtree_node_count_before_delta() {
    const GROUP_KEY: Key = 364_003;

    let mut table = SlotTable::new();
    let owner = table.insert_new_group(
        ChildCursor::new(AnchorId::INVALID, 0),
        GroupKey::new(GROUP_KEY, None, 0),
    );
    table.groups[0].subtree_node_count = u32::MAX;

    let update = table.record_node_at_cursor(owner, 0, 42, None, 1, crate::slot::BRANCH_PATH_ROOT);

    assert_eq!(
        update,
        NodeSlotUpdate::Inserted {
            id: 42,
            generation: 1,
        }
    );
    assert_eq!(table.groups[0].subtree_node_count, 1);
    assert_eq!(table.total_node_count(), 1);
    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn node_tail_removal_repairs_corrupt_subtree_node_count_before_delta() {
    const GROUP_KEY: Key = 364_004;

    let mut table = SlotTable::new();
    let owner = table.insert_new_group(
        ChildCursor::new(AnchorId::INVALID, 0),
        GroupKey::new(GROUP_KEY, None, 0),
    );
    let update = table.record_node_at_cursor(owner, 0, 42, None, 1, crate::slot::BRANCH_PATH_ROOT);
    assert!(matches!(update, NodeSlotUpdate::Inserted { .. }));
    table.groups[0].subtree_node_count = 0;

    let removed = table.remove_group_node_tail_at_cursor(owner, 0);

    assert_eq!(removed.len(), 1);
    assert_eq!(table.groups[0].subtree_node_count, 0);
    assert_eq!(table.total_node_count(), 0);
    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn node_tail_range_with_corrupt_segment_start_is_empty() {
    const GROUP_KEY: Key = 364_005;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        session.record_node_with_parent(42, 1, None, crate::slot::BRANCH_PATH_ROOT);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.table.groups[0].node_start = u32::MAX;

    let range = harness.table.group_node_tail_range_at(0, 0);

    assert!(range.is_empty());
    assert_eq!(harness.table.total_node_count(), 1);
}

#[test]
fn node_range_outside_current_group_segment_is_ignored() {
    const GROUP_KEY: Key = 364_006;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        session.record_node_with_parent(42, 1, None, crate::slot::BRANCH_PATH_ROOT);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let stale_range =
        crate::slot::ranges::GroupItemRange::new(0, crate::slot::NodeRange::new(0, 2), 0, 2);
    let removed = harness.table.remove_group_node_range(stale_range);

    assert!(removed.is_empty());
    assert_eq!(harness.table.total_node_count(), 1);
    assert_eq!(harness.table.group_node_record_at(0, 0).id, 42);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn collecting_root_nodes_with_stale_group_anchor_returns_empty() {
    const GROUP_KEY: Key = 364_001;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let group_anchor = harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        session.record_node_with_parent(42, 1, None, crate::slot::BRANCH_PATH_ROOT);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        started.anchor
    });
    harness.finish_pass();

    harness.table.anchors.mark_detached(group_anchor);

    assert!(harness
        .table
        .collect_subtree_root_node_ids(group_anchor)
        .is_empty());
}

#[test]
fn collecting_root_nodes_repairs_corrupt_subtree_span_and_node_count() {
    const PARENT_KEY: Key = 364_005;
    const CHILD_KEY: Key = 364_006;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        session.record_node_with_parent(10, 1, None, crate::slot::BRANCH_PATH_ROOT);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(11, 1, Some(10), crate::slot::BRANCH_PATH_ROOT);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        parent.anchor
    });
    harness.finish_pass();

    harness.table.groups[0].subtree_len = 3;
    harness.table.groups[0].subtree_node_count = 10;

    let root_nodes = harness.table.collect_subtree_root_node_ids(parent_anchor);

    assert_eq!(root_nodes, vec![10]);
    assert_eq!(
        harness.table.groups[0].subtree_len, 2,
        "root-node collection should repair subtree span before iterating"
    );
    assert_eq!(
        harness.table.groups[0].subtree_node_count, 2,
        "root-node collection should repair its reserve/count metadata"
    );
}

#[test]
fn record_node_reports_explicit_insert_and_reuse() {
    const GROUP_KEY: Key = 365;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node_with_parent(11, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Inserted {
                id: 11,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(
            session.current_node_record(),
            Some((11, 1, crate::slot::BRANCH_PATH_ROOT))
        );
        let recorded = session.record_node_with_parent(11, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Reused {
                id: 11,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 11);
    assert_eq!(harness.table.group_node_record_at(0, 0).generation, 1);
}

#[test]
fn record_node_inserts_on_id_change_and_trims_the_stale_record() {
    const GROUP_KEY: Key = 366;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node_with_parent(11, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Inserted {
                id: 11,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(
            session.current_node_record(),
            Some((11, 1, crate::slot::BRANCH_PATH_ROOT))
        );
        let recorded = session.record_node_with_parent(12, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Inserted {
                id: 12,
                generation: 1,
            },
            "a different node id at the cursor inserts; the stale record is trimmed at finish",
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert_eq!(
            result.direct_nodes,
            vec![11],
            "the stale node must leave through the finish tail trim",
        );
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 12);
    assert_eq!(harness.table.group_node_record_at(0, 0).generation, 1);
    assert_eq!(harness.table.group_node_len_at(0), 1);
}

#[test]
fn record_node_reports_explicit_generation_replacement() {
    const GROUP_KEY: Key = 367;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node_with_parent(11, 1, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Inserted {
                id: 11,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(
            session.current_node_record(),
            Some((11, 1, crate::slot::BRANCH_PATH_ROOT))
        );
        let recorded = session.record_node_with_parent(11, 2, None, crate::slot::BRANCH_PATH_ROOT);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Replaced {
                old_id: 11,
                old_generation: 1,
                new_id: 11,
                new_generation: 2,
            },
            "generation changes at the same node id must report explicit replacement",
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 11);
    assert_eq!(harness.table.group_node_record_at(0, 0).generation, 2);
}
