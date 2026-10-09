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
    assert!(
        harness
            .table
            .remove_group_node_tail_at_cursor(group_anchor, 0)
            .is_empty()
    );
    assert_eq!(harness.table.total_node_count(), 0);
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

fn node_record(id: NodeId, parent_id: Option<NodeId>) -> NodeRecord {
    NodeRecord {
        owner: AnchorId::INVALID,
        id,
        parent_id,
        generation: 0,
        source: crate::slot::BRANCH_PATH_ROOT,
        lifecycle: NodeLifecycle::Active,
    }
}

#[test]
fn root_node_ids_are_the_records_whose_parent_is_outside_them_in_order() {
    // Chains of three under outside parents: a root, its child and its
    // grandchild, repeated until the records pass the scan limit.
    for chains in [1, 2, 10] {
        let records: Vec<NodeRecord> = (0..chains)
            .flat_map(|chain| {
                let root = 100 + chain * 3;
                [
                    node_record(root, (chain % 2 == 0).then_some(1)),
                    node_record(root + 1, Some(root)),
                    node_record(root + 2, Some(root + 1)),
                ]
            })
            .collect();
        let expected: Vec<NodeId> = (0..chains).map(|chain| 100 + chain * 3).collect();
        assert_eq!(
            crate::slot::types::root_node_ids(&records).collect::<Vec<_>>(),
            expected,
            "{} records",
            records.len()
        );
    }
}
