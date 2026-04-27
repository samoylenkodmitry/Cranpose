use super::*;

#[test]
fn validate_reports_duplicate_sibling_key_structurally() {
    const PARENT_KEY: Key = 462;
    const STATIC_KEY: Key = 463;

    let mut table = SlotTable::new();
    let mut lifecycle = SlotLifecycleCoordinator::default();
    let mut state = SlotWriteSessionState::default();
    state.reset_for_pass(SlotPassMode::Compose);
    {
        let mut session = table.write_session(&mut lifecycle, &mut state);
        begin_unkeyed(&mut session, PARENT_KEY, None);

        begin_keyed(&mut session, STATIC_KEY, 1, None);
        let first = session.finish_group_body();
        assert!(first.detached_children.is_empty());
        session.end_group();

        begin_keyed(&mut session, STATIC_KEY, 2, None);
        let second = session.finish_group_body();
        assert!(second.detached_children.is_empty());
        session.end_group();

        let parent = session.finish_group_body();
        assert!(parent.detached_children.is_empty());
        session.end_group();
    }

    table.groups[2].key = table.groups[1].key;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::DuplicateSiblingKey {
            parent_anchor: table.groups[0].anchor,
            key: table.groups[1].key,
        })
    );
}

#[test]
fn validate_reports_invalid_parent_structurally() {
    let mut table = composed_parent_child_table(470, 471, None);
    table.groups[1].parent_anchor = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::InvalidParent {
            group_index: 1,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_bad_subtree_len_structurally() {
    let mut table = composed_parent_child_table(472, 473, None);
    table.groups[1].subtree_len = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadSubtreeLen {
            group_index: 1,
            expected: 0,
            actual: 0,
        })
    );
}

#[test]
fn validate_reports_scope_index_mismatch_structurally() {
    const SCOPE_ID: ScopeId = 64;
    const STALE_SCOPE_ID: ScopeId = 65;

    let mut table = composed_parent_child_table(474, 475, Some(SCOPE_ID));
    table.groups[1].scope_id = Some(STALE_SCOPE_ID);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexMismatch {
            scope_id: STALE_SCOPE_ID,
            expected: table.groups[1].anchor,
            actual: None,
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_wrong_active_index_structurally() {
    let mut table = composed_parent_child_table(476, 477, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.set_active(root_anchor, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: Some(AnchorState::Active(1)),
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_detached_anchor_structurally() {
    let mut table = composed_parent_child_table(487, 488, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.mark_detached(root_anchor);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: Some(AnchorState::Detached),
        })
    );
}

#[test]
fn validate_reports_group_anchor_count_mismatch_structurally() {
    let mut table = composed_parent_child_table(480, 481, None);
    table.anchors.set_active(AnchorId::new(2_000), 9);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::GroupAnchorCountMismatch {
            expected: 2,
            actual: 3,
        })
    );
}

#[test]
fn validate_reports_anchor_mismatch_for_missing_anchor_structurally() {
    let mut table = composed_parent_child_table(494, 495, None);
    let root_anchor = table.groups[0].anchor;
    table.anchors.invalidate(root_anchor);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::AnchorMismatch {
            anchor: root_anchor,
            expected: 0,
            actual: None,
        })
    );
}

#[test]
fn validate_reports_bad_depth_structurally() {
    let mut table = composed_parent_child_table(489, 490, None);
    table.groups[1].depth = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadDepth {
            group_index: 1,
            expected: 1,
            actual: 0,
        })
    );
}

#[test]
fn validate_reports_payload_owner_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(478);
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;
    table.group_payload_record_at_mut(0, 0).owner = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadOwnerMismatch {
            payload_anchor,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_payload_start_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(481);
    table.groups[0].payload_start = 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadStartMismatch {
            group_index: 0,
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_payload_out_of_range_structurally() {
    let mut table = composed_group_with_value_and_node_table(484);
    table.groups[0].payload_len = 2;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadOutOfRange {
            group_index: 0,
            start: 0,
            len: 2,
            payload_count: 1,
        })
    );
}

#[test]
fn validate_reports_payload_count_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(488);
    let owner = table.groups[0].anchor;
    let extra_anchor = 10_001;

    table.payloads.push(super::PayloadRecord {
        owner,
        anchor: extra_anchor,
        generation: 1,
        type_id: TypeId::of::<i32>(),
        type_name: std::any::type_name::<i32>(),
        kind: super::PayloadKind::Internal,
        value: Box::new(0_i32),
    });
    table.payload_locations.insert(extra_anchor, owner, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadCountMismatch {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn validate_reports_payload_location_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(482);
    let stale_payload_anchor = table.group_payload_record_at(0, 0).anchor;
    table.group_payload_record_at_mut(0, 0).anchor = stale_payload_anchor + 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadLocationMismatch {
            payload_anchor: stale_payload_anchor + 1,
            expected: (table.groups[0].anchor, 0),
            actual: None,
        })
    );
}

#[test]
fn validate_reports_payload_location_stale_owner_structurally() {
    let mut table = composed_group_with_value_and_node_table(491);
    let old_anchor = table.groups[0].anchor;
    let new_anchor = AnchorId::new(1_001);
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;

    table.groups[0].anchor = new_anchor;
    table.group_payload_record_at_mut(0, 0).owner = new_anchor;
    table.anchors.invalidate(old_anchor);
    table.anchors.set_active(new_anchor, 0);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadLocationMismatch {
            payload_anchor,
            expected: (new_anchor, 0),
            actual: Some((old_anchor, 0)),
        })
    );
}

#[test]
fn validate_reports_payload_anchor_count_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(483);
    table
        .payload_locations
        .insert(999, table.groups[0].anchor, 0);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadAnchorCountMismatch {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn compact_storage_discards_removed_payload_location_entries() {
    let mut table = composed_group_with_value_and_node_table(601);
    let owner = table.groups[0].anchor;
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;

    let payload_range = table.group_payload_subrange_at(0, 0, 1);
    let removed = table.remove_payload_range(owner, payload_range);
    assert_eq!(removed.len(), 1);
    assert_eq!(table.payload_locations.get(payload_anchor), None);

    let capacity_before = table.payload_locations.capacity();
    table.compact_storage();
    let capacity_after = table.payload_locations.capacity();

    assert_eq!(table.payload_locations.len(), 0);
    assert!(
        capacity_after < capacity_before,
        "compaction must drop removed payload-location entries: before={capacity_before} after={capacity_after}",
    );
    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn node_tail_range_past_group_end_removes_nothing() {
    let mut table = composed_group_with_value_and_node_table(602);
    let node_count = table.group_node_len_at(0);
    let range = table.group_node_tail_range_at(0, node_count + 3);

    let removed = table.remove_group_node_range(range);

    assert!(removed.is_empty());
    assert_eq!(table.group_node_len_at(0), node_count);
    assert_eq!(table.total_node_count(), node_count);
    assert_eq!(table.validate(), Ok(()));
}

#[test]
fn compact_payload_namespace_remaps_retained_subtree_payloads() {
    const PARENT_KEY: Key = 610;
    const CHILD_KEY: Key = 611;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let _ = session.value_slot(|| 10_i32);

        begin_unkeyed(session, CHILD_KEY, None);
        let _ = session.value_slot(|| 20_i32);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        parent.anchor
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

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let restore_key = detached.root_key();
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);

    harness
        .table
        .compact_payload_anchor_namespace(Some(&mut retention));
    let _ =
        harness
            .table
            .insert_value_payload(parent_anchor, 0, super::PayloadKind::Internal, 30_i32);

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness
        .table
        .restore_subtree(1, parent_anchor, restore_key, restored);

    let payload_anchor_count = harness
        .table
        .payloads
        .iter()
        .map(|payload| payload.anchor)
        .collect::<HashSet<_>>()
        .len();
    assert_eq!(payload_anchor_count, harness.table.payloads.len());
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn validate_reports_node_owner_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(479);
    let node_id = table.group_node_record_at(0, 0).id;
    table.group_node_record_at_mut(0, 0).owner = AnchorId::INVALID;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeOwnerMismatch {
            node_id,
            expected: table.groups[0].anchor,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn validate_reports_duplicate_node_id_structurally() {
    let mut table = composed_group_with_value_and_node_table(480);
    let node = table.group_node_record_at(0, 0).id;
    table.nodes.push(super::NodeRecord {
        owner: table.groups[0].anchor,
        id: node,
        parent_id: None,
        generation: 2,
        lifecycle: super::NodeLifecycle::Active,
    });
    table.groups[0].node_len = 2;
    table.groups[0].subtree_node_count = 2;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::DuplicateNodeId { node_id: node })
    );
}

#[test]
fn validate_reports_node_lifecycle_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(484);
    let node_id = table.group_node_record_at(0, 0).id;
    table.group_node_record_at_mut(0, 0).lifecycle = super::NodeLifecycle::Disposed;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeLifecycleMismatch {
            node_id,
            expected: super::NodeLifecycle::Active,
            actual: super::NodeLifecycle::Disposed,
        })
    );
}

#[test]
fn validate_reports_node_start_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(485);
    table.groups[0].node_start = 1;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeStartMismatch {
            group_index: 0,
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_node_out_of_range_structurally() {
    let mut table = composed_group_with_value_and_node_table(486);
    table.groups[0].node_len = 2;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeOutOfRange {
            group_index: 0,
            start: 0,
            len: 2,
            node_count: 1,
        })
    );
}

#[test]
fn validate_reports_node_count_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(487);
    table.nodes.push(super::NodeRecord {
        owner: table.groups[0].anchor,
        id: 999,
        parent_id: None,
        generation: 1,
        lifecycle: super::NodeLifecycle::Active,
    });

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeCountMismatch {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn validate_reports_scope_index_stale_anchor_structurally() {
    const SCOPE_ID: ScopeId = 67;

    let mut table = composed_parent_child_table(492, 493, Some(SCOPE_ID));
    let old_anchor = table.groups[1].anchor;
    let new_anchor = AnchorId::new(1_002);

    table.groups[1].anchor = new_anchor;
    table.anchors.invalidate(old_anchor);
    table.anchors.set_active(new_anchor, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexMismatch {
            scope_id: SCOPE_ID,
            expected: new_anchor,
            actual: Some(old_anchor),
        })
    );
}

#[test]
fn validate_reports_scope_index_count_mismatch_structurally() {
    const SCOPE_ID: ScopeId = 66;

    let mut table = composed_parent_child_table(484, 485, Some(SCOPE_ID));
    table.groups[1].scope_id = None;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::ScopeIndexCountMismatch {
            expected: 0,
            actual: 1,
        })
    );
}

#[test]
fn validate_reports_bad_subtree_node_count_structurally() {
    let mut table = composed_group_with_value_and_node_table(486);
    table.groups[0].subtree_node_count = 0;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::BadSubtreeNodeCount {
            group_index: 0,
            expected: 1,
            actual: 0,
        })
    );
}
