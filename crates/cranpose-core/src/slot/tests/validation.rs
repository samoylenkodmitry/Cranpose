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
            tree: SlotTreeContext::Active,
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
            tree: SlotTreeContext::Active,
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
    table.anchors.clear();

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
            tree: SlotTreeContext::Active,
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
            tree: SlotTreeContext::Active,
            payload_anchor: payload_anchor.id(),
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
            tree: SlotTreeContext::Active,
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
            tree: SlotTreeContext::Active,
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
    let extra_anchor = PayloadAnchor::new(10_001, 1);

    table.payloads.push(super::PayloadRecord {
        owner,
        anchor: extra_anchor,
        type_id: TypeId::of::<i32>(),
        type_name: std::any::type_name::<i32>(),
        kind: super::PayloadKind::Internal,
        value: Box::new(0_i32),
    });
    table.payload_locations.insert(extra_anchor, owner, 1);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadCountMismatch {
            tree: SlotTreeContext::Active,
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn validate_reports_payload_anchor_registry_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(482);
    let stale_payload_anchor = table.group_payload_record_at(0, 0).anchor;
    let mismatched_payload_anchor = PayloadAnchor::new(
        stale_payload_anchor.id() + 1,
        stale_payload_anchor.generation(),
    );
    table.group_payload_record_at_mut(0, 0).anchor = mismatched_payload_anchor;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadAnchorRegistryMismatch {
            payload_anchor: mismatched_payload_anchor,
            expected: (table.groups[0].anchor, 0),
            actual: None,
        })
    );
}

#[test]
fn validate_reports_payload_anchor_registry_stale_owner_structurally() {
    let mut table = composed_group_with_value_and_node_table(491);
    let old_anchor = table.groups[0].anchor;
    let new_anchor = AnchorId::new(1_001);
    let payload_anchor = table.group_payload_record_at(0, 0).anchor;

    table.groups[0].anchor = new_anchor;
    table.group_payload_record_at_mut(0, 0).owner = new_anchor;
    table.anchors.mark_detached(old_anchor);
    table.anchors.set_active(new_anchor, 0);

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::PayloadAnchorRegistryMismatch {
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
        .insert(PayloadAnchor::new(999, 1), table.groups[0].anchor, 0);

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
fn compact_anchor_namespace_preserves_active_group_anchors() {
    const STATIC_KEY: Key = 603;
    const GROUP_COUNT: usize = 1_100;
    const RETAINED_EXPLICIT_KEY: Key = (GROUP_COUNT - 1) as Key;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        for explicit_key in 0..GROUP_COUNT as Key {
            let group = begin_keyed(session, STATIC_KEY, explicit_key, None);
            assert_eq!(group.kind, GroupStartKind::Inserted);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let retained_anchor = harness.session(|session| {
        let group = begin_keyed(session, STATIC_KEY, RETAINED_EXPLICIT_KEY, None);
        assert_eq!(group.kind, GroupStartKind::Moved);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        group.anchor
    });
    harness.finish_pass();

    assert_eq!(harness.table.groups.len(), 1);
    assert!(
        retained_anchor.id as usize > 1_024,
        "test must exercise a sparse active anchor namespace"
    );
    let snapshot_before = harness.identity_snapshot(None, &[]);
    let group_keys_before = harness
        .table
        .groups
        .iter()
        .map(|group| (group.anchor, group.key))
        .collect::<Vec<_>>();

    harness.table.compact_anchor_namespace(None, |_| None);

    let snapshot_after = harness.identity_snapshot(None, &[]);
    assert_eq!(
        snapshot_after.active_group_anchors,
        snapshot_before.active_group_anchors
    );
    assert_eq!(
        harness.table.group_anchor_state(retained_anchor),
        Some(AnchorState::Active(0))
    );
    assert_eq!(
        harness
            .table
            .groups
            .iter()
            .map(|group| (group.anchor, group.key))
            .collect::<Vec<_>>(),
        group_keys_before
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn compact_anchor_namespace_preserves_retained_group_anchors() {
    const PARENT_KEY: Key = 604;
    const CHILD_STATIC_KEY: Key = 605;
    const GROUP_COUNT: usize = 1_100;
    const RETAINED_EXPLICIT_KEY: Key = (GROUP_COUNT - 1) as Key;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in 0..GROUP_COUNT as Key {
            let child = begin_keyed(session, CHILD_STATIC_KEY, explicit_key, None);
            assert_eq!(child.kind, GroupStartKind::Inserted);
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        parent.anchor
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached_children = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let result = session.finish_group_body();
        session.end_group();
        result.detached_children
    });
    harness.finish_pass();

    let mut retained = None;
    for subtree in detached_children {
        if subtree.root_key().explicit_key == Some(RETAINED_EXPLICIT_KEY) {
            retained = Some(subtree);
        } else {
            harness.table.invalidate_detached_subtree_anchors(&subtree);
            harness.lifecycle.queue_subtree_disposal(subtree);
        }
    }
    harness.lifecycle.flush_pending_drops();
    let retained = retained.expect("target child subtree must detach");
    let retained_group_anchors = retained.group_anchors().collect::<Vec<_>>();
    assert_eq!(retained_group_anchors.len(), 1);
    assert!(
        retained_group_anchors[0].id as usize > 1_024,
        "test must exercise a sparse retained anchor namespace"
    );

    let retain_key = RetainKey {
        parent_scope: None,
        key: retained.root_key(),
    };
    let restore_key = retained.root_key();
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, retained);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let snapshot_before = harness.identity_snapshot(Some(&retention), &[]);
    harness
        .table
        .compact_anchor_namespace(Some(&mut retention), |_| None);
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let snapshot_after = harness.identity_snapshot(Some(&retention), &[]);
    assert_eq!(
        snapshot_after.retained_group_anchors,
        snapshot_before.retained_group_anchors
    );

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness
        .table
        .restore_subtree(1, parent_anchor, restore_key, restored);
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(
        harness.table.group_anchor_state(retained_group_anchors[0]),
        Some(AnchorState::Active(1))
    );
    assert_eq!(
        harness.identity_snapshot(None, &[]).active_group_anchors,
        vec![parent_anchor, retained_group_anchors[0]]
    );
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
fn compact_payload_namespace_preserves_active_value_slots() {
    const PARENT_KEY: Key = 606;
    const CHILD_STATIC_KEY: Key = 607;
    const GROUP_COUNT: usize = 1_100;
    const KEPT_EXPLICIT_KEY: Key = (GROUP_COUNT - 1) as Key;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, retained_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let mut retained_slot = None;
        for explicit_key in 0..GROUP_COUNT as Key {
            let child = begin_keyed(session, CHILD_STATIC_KEY, explicit_key, None);
            assert_eq!(child.kind, GroupStartKind::Inserted);
            let slot = session.value_slot_with_kind(PayloadKind::Internal, || explicit_key as i32);
            if explicit_key == KEPT_EXPLICIT_KEY {
                retained_slot = Some(slot);
            }
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (
            parent.anchor,
            retained_slot.expect("target value slot must be captured"),
        )
    });
    harness.finish_pass();
    assert!(
        retained_slot.anchor().id() > 1_024,
        "test must exercise a sparse active payload namespace"
    );

    harness.begin_pass(SlotPassMode::Compose);
    let moved_slot = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let child = begin_keyed(session, CHILD_STATIC_KEY, KEPT_EXPLICIT_KEY, None);
        assert_eq!(child.kind, GroupStartKind::Moved);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        session.end_group();
        assert_eq!(result.detached_children.len(), GROUP_COUNT - 1);
        slot
    });
    harness.finish_pass();

    assert_eq!(moved_slot, retained_slot);
    assert_eq!(
        *harness.table.read_value::<i32>(retained_slot),
        KEPT_EXPLICIT_KEY as i32
    );
    let snapshot_before = harness.identity_snapshot(None, &[retained_slot]);
    harness.table.compact_payload_anchor_namespace(None);
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(
        *harness.table.read_value::<i32>(retained_slot),
        KEPT_EXPLICIT_KEY as i32
    );
    assert_eq!(
        harness
            .identity_snapshot(None, &[retained_slot])
            .active_payload_anchors,
        snapshot_before.active_payload_anchors
    );
}

#[test]
fn compact_payload_namespace_preserves_retained_value_slots() {
    const PARENT_KEY: Key = 608;
    const CHILD_STATIC_KEY: Key = 609;
    const GROUP_COUNT: usize = 1_100;
    const RETAINED_EXPLICIT_KEY: Key = (GROUP_COUNT - 1) as Key;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, retained_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let mut retained_slot = None;
        for explicit_key in 0..GROUP_COUNT as Key {
            let child = begin_keyed(session, CHILD_STATIC_KEY, explicit_key, None);
            assert_eq!(child.kind, GroupStartKind::Inserted);
            let slot =
                session.value_slot_with_kind(PayloadKind::Internal, || (explicit_key as i32) * 10);
            if explicit_key == RETAINED_EXPLICIT_KEY {
                retained_slot = Some(slot);
            }
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (
            parent.anchor,
            retained_slot.expect("target retained value slot must be captured"),
        )
    });
    harness.finish_pass();
    assert!(
        retained_slot.anchor().id() > 1_024,
        "test must exercise a sparse retained payload namespace"
    );

    harness.begin_pass(SlotPassMode::Compose);
    let detached_children = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let result = session.finish_group_body();
        session.end_group();
        result.detached_children
    });
    harness.finish_pass();

    let mut retained = None;
    for subtree in detached_children {
        if subtree.root_key().explicit_key == Some(RETAINED_EXPLICIT_KEY) {
            retained = Some(subtree);
        } else {
            harness.table.invalidate_detached_subtree_anchors(&subtree);
            harness.lifecycle.queue_subtree_disposal(subtree);
        }
    }
    harness.lifecycle.flush_pending_drops();
    let retained = retained.expect("target child subtree must detach");
    let retain_key = RetainKey {
        parent_scope: None,
        key: retained.root_key(),
    };
    let restore_key = retained.root_key();
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, retained);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let retained_identity = PayloadIdentity::from(retained_slot);
    let snapshot_before = harness.identity_snapshot(Some(&retention), &[retained_slot]);
    assert_eq!(
        snapshot_before.retained_payload_anchors,
        vec![retained_identity]
    );
    harness
        .table
        .compact_payload_anchor_namespace(Some(&mut retention));
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(retention.validate(&harness.table), Ok(()));
    assert_eq!(
        harness
            .identity_snapshot(Some(&retention), &[retained_slot])
            .retained_payload_anchors,
        snapshot_before.retained_payload_anchors
    );

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness
        .table
        .restore_subtree(1, parent_anchor, restore_key, restored);
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(
        *harness.table.read_value::<i32>(retained_slot),
        (RETAINED_EXPLICIT_KEY as i32) * 10
    );
    assert_eq!(
        harness
            .identity_snapshot(None, &[retained_slot])
            .active_payload_anchors,
        vec![retained_identity]
    );
}

#[test]
fn compact_payload_namespace_preserves_retained_payload_uniqueness() {
    const PARENT_KEY: Key = 610;
    const CHILD_KEY: Key = 611;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);

        begin_unkeyed(session, CHILD_KEY, None);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
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
fn disposed_identities_reuse_only_after_generation_bump() {
    const PARENT_KEY: Key = 612;
    const CHILD_STATIC_KEY: Key = 613;
    const ACTIVE_KEY: Key = 1;
    const RETAINED_KEY: Key = 2;
    const DISPOSED_KEY: Key = 3;
    const NEW_KEY: Key = 4;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (
        parent_anchor,
        active_anchor,
        active_slot,
        retained_anchor,
        retained_slot,
        disposed_anchor,
        disposed_slot,
    ) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);

        let active = begin_keyed(session, CHILD_STATIC_KEY, ACTIVE_KEY, None);
        let active_slot = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let retained = begin_keyed(session, CHILD_STATIC_KEY, RETAINED_KEY, None);
        let retained_slot = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let disposed = begin_keyed(session, CHILD_STATIC_KEY, DISPOSED_KEY, None);
        let disposed_slot = session.value_slot_with_kind(PayloadKind::Internal, || 30_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (
            parent.anchor,
            active.anchor,
            active_slot,
            retained.anchor,
            retained_slot,
            disposed.anchor,
            disposed_slot,
        )
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached_children = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let active = begin_keyed(session, CHILD_STATIC_KEY, ACTIVE_KEY, None);
        assert_eq!(active.anchor, active_anchor);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
        assert_eq!(slot, active_slot);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        session.end_group();
        result.detached_children
    });
    harness.finish_pass();

    let mut retained = None;
    for subtree in detached_children {
        match subtree.root_key().explicit_key {
            Some(RETAINED_KEY) => retained = Some(subtree),
            Some(DISPOSED_KEY) => {
                harness.table.invalidate_detached_subtree_anchors(&subtree);
                harness.lifecycle.queue_subtree_disposal(subtree);
            }
            actual => panic!("unexpected detached child key: {actual:?}"),
        }
    }
    harness.lifecycle.flush_pending_drops();

    let retain_key = RetainKey {
        parent_scope: None,
        key: retained
            .as_ref()
            .expect("retained child must detach")
            .root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(
        retain_key,
        retained.expect("retained child must be captured"),
    );
    assert_eq!(
        harness.table.anchor_state(retained_anchor),
        Some(AnchorState::Detached)
    );
    assert_eq!(harness.table.anchor_state(disposed_anchor), None);

    let disposed_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(disposed_slot);
    }));
    assert!(disposed_read.is_err());

    harness.begin_pass(SlotPassMode::Compose);
    let (new_anchor, new_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let active = begin_keyed(session, CHILD_STATIC_KEY, ACTIVE_KEY, None);
        assert_eq!(active.anchor, active_anchor);
        let active_slot_after = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
        assert_eq!(active_slot_after, active_slot);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let new_child = begin_keyed(session, CHILD_STATIC_KEY, NEW_KEY, None);
        assert_eq!(new_child.kind, GroupStartKind::Inserted);
        let new_slot = session.value_slot_with_kind(PayloadKind::Internal, || 40_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (new_child.anchor, new_slot)
    });
    harness.finish_pass();

    assert_eq!(new_anchor.id, disposed_anchor.id);
    assert_ne!(new_anchor.generation, disposed_anchor.generation);
    assert_ne!(new_anchor.id, active_anchor.id);
    assert_ne!(new_anchor.id, retained_anchor.id);
    assert_eq!(
        harness.table.anchor_state(new_anchor),
        Some(AnchorState::Active(2))
    );
    assert_eq!(harness.table.anchor_state(disposed_anchor), None);
    assert_ne!(new_slot.anchor(), active_slot.anchor());
    assert_ne!(new_slot.anchor(), retained_slot.anchor());
    assert_ne!(new_slot.anchor(), disposed_slot.anchor());

    for subtree in retention.into_subtrees() {
        harness.table.invalidate_detached_subtree_anchors(&subtree);
        harness.lifecycle.queue_subtree_disposal(subtree);
    }
    harness.lifecycle.flush_pending_drops();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();
    assert!(harness.table.groups.is_empty());
    harness.table.compact_payload_anchor_namespace(None);

    harness.begin_pass(SlotPassMode::Compose);
    let reused_payload_slot = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY + 1, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 50_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(reused_payload_slot.anchor().id(), active_slot.anchor().id());
    assert_ne!(
        reused_payload_slot.anchor().generation(),
        active_slot.anchor().generation()
    );
    assert_eq!(*harness.table.read_value::<i32>(reused_payload_slot), 50);

    let stale_active_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(active_slot);
    }));
    assert!(stale_active_read.is_err());
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
            tree: SlotTreeContext::Active,
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
        Err(SlotInvariantError::DuplicateNodeId {
            tree: SlotTreeContext::Active,
            node_id: node,
        })
    );
}

#[test]
fn validate_reports_node_lifecycle_mismatch_structurally() {
    let mut table = composed_group_with_value_and_node_table(484);
    let node_id = table.group_node_record_at(0, 0).id;
    table.group_node_record_at_mut(0, 0).lifecycle = super::NodeLifecycle::RetainedDetached;

    assert_eq!(
        table.validate(),
        Err(SlotInvariantError::NodeLifecycleMismatch {
            node_id,
            expected: super::NodeLifecycle::Active,
            actual: super::NodeLifecycle::RetainedDetached,
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
            tree: SlotTreeContext::Active,
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
            tree: SlotTreeContext::Active,
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
            tree: SlotTreeContext::Active,
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
    table.anchors.mark_detached(old_anchor);
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
            tree: SlotTreeContext::Active,
            group_index: 0,
            expected: 1,
            actual: 0,
        })
    );
}
