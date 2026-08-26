use super::*;

#[test]
fn perf_large_keyed_reorder_avoids_identity_rebuilds() {
    const PARENT_KEY: Key = 8_000;
    const CHILD_KEY: Key = 8_001;
    const CHILD_COUNT: Key = 64;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in 0..CHILD_COUNT {
            begin_keyed(session, CHILD_KEY, explicit_key, None);
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                || explicit_key as i32,
            );
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in (0..CHILD_COUNT).rev() {
            let child = begin_keyed(session, CHILD_KEY, explicit_key, None);
            assert!(matches!(
                child.kind,
                GroupStartKind::Moved | GroupStartKind::Reused
            ));
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                || -1_i32,
            );
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let after = harness.table.debug_stats().mutation;
    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        0
    );
    assert!(
        after.subtree_move_count - before.subtree_move_count <= CHILD_COUNT as usize,
        "keyed reorder must not perform more moves than moved children"
    );
    assert!(
        after.group_index_refresh_count - before.group_index_refresh_count <= CHILD_COUNT as usize,
        "keyed reorder must not rebuild group anchors outside per-move refreshes"
    );
}

#[test]
fn perf_deep_insert_remove_avoids_full_identity_rebuilds() {
    const ROOT_KEY: Key = 8_010;
    const CHILD_KEY_BASE: Key = 8_100;
    const DEPTH: usize = 48;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before_insert = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        for depth in 0..DEPTH {
            begin_unkeyed(session, CHILD_KEY_BASE + depth as Key, None);
        }
        for _ in 0..DEPTH {
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let after_insert = harness.table.debug_stats().mutation;
    assert_eq!(
        after_insert.payload_location_range_refresh_count
            - before_insert.payload_location_range_refresh_count,
        0
    );

    let before_remove = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        let result = session.finish_group_body();
        session.end_group();
        result.detached_children
    });
    harness.finish_pass();

    assert_eq!(detached.len(), 1);
    let after_remove = harness.table.debug_stats().mutation;
    assert_eq!(
        after_remove.group_index_refresh_count - before_remove.group_index_refresh_count,
        1
    );
    assert_eq!(
        after_remove.payload_location_range_refresh_count
            - before_remove.payload_location_range_refresh_count,
        0
    );
}

#[test]
fn perf_retained_restore_refreshes_only_restored_payload_range() {
    const PARENT_KEY: Key = 8_020;
    const CHILD_KEY: Key = 8_021;
    const CHILD_SCOPE: ScopeId = 8_022;

    let (mut harness, detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, Some(CHILD_SCOPE), true, true);
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, Some(restored));
        assert_eq!(child.kind, GroupStartKind::Restored);
        assert_eq!(child.scope_id, Some(CHILD_SCOPE));
        let _ = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || -1_i32,
        );
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let after = harness.table.debug_stats().mutation;
    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        1
    );
    assert_eq!(
        after.payload_location_range_refresh_payload_count
            - before.payload_location_range_refresh_payload_count,
        1
    );
}

#[test]
fn perf_large_retained_subtree_restore_preserves_exact_ranges() {
    const PARENT_KEY: Key = 8_050;
    const CHILD_KEY: Key = 8_051;
    const GRANDCHILD_KEY: Key = 8_052;
    const CHILD_SCOPE: ScopeId = 8_053;
    const GRANDCHILD_SCOPE_BASE: ScopeId = 8_100;
    const CHILD_NODE: NodeId = 80_500;
    const GRANDCHILD_NODE_BASE: NodeId = 80_600;
    const GRANDCHILD_COUNT: usize = 48;
    const RESTORED_GROUP_COUNT: usize = GRANDCHILD_COUNT + 1;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_anchor, child_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let child_slot = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || 77_i32,
        );
        session.record_node_with_parent(CHILD_NODE, 1, None);
        for index in 0..GRANDCHILD_COUNT {
            let grandchild = begin_keyed(session, GRANDCHILD_KEY, index as Key, None);
            session.set_group_scope(grandchild.group, GRANDCHILD_SCOPE_BASE + index as ScopeId);
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || index as i32,
            );
            session.record_node_with_parent(
                GRANDCHILD_NODE_BASE + index as NodeId,
                1,
                Some(CHILD_NODE),
            );
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        (parent.anchor, child.anchor, child_slot)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let result = session.finish_group_body();
        session.end_group();
        assert_eq!(result.detached_children.len(), 1);
        result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    assert_eq!(detached.group_count(), RESTORED_GROUP_COUNT);
    assert_eq!(detached.payload_count(), RESTORED_GROUP_COUNT);
    assert_eq!(detached.node_count(), RESTORED_GROUP_COUNT);
    assert_eq!(
        detached.groups[0].subtree_len as usize,
        RESTORED_GROUP_COUNT
    );
    assert_eq!(
        detached.groups[0].subtree_node_count as usize,
        RESTORED_GROUP_COUNT
    );

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    let restored = retention
        .take(retain_key)
        .expect("large retained subtree must restore");

    let before = harness.table.debug_stats().mutation;
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let child = begin_unkeyed(session, CHILD_KEY, Some(restored));
        assert_eq!(child.kind, GroupStartKind::Restored);
        assert_eq!(child.anchor, child_anchor);
        assert_eq!(child.scope_id, Some(CHILD_SCOPE));
        let restored_child_slot = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || -1_i32,
        );
        assert_eq!(restored_child_slot, child_slot);
        assert_eq!(
            session.record_node_with_parent(CHILD_NODE, 1, None),
            NodeSlotUpdate::Reused {
                id: CHILD_NODE,
                generation: 1,
            }
        );
        for index in 0..GRANDCHILD_COUNT {
            let grandchild = begin_keyed(session, GRANDCHILD_KEY, index as Key, None);
            assert_eq!(grandchild.kind, GroupStartKind::Reused);
            assert_eq!(
                grandchild.scope_id,
                Some(GRANDCHILD_SCOPE_BASE + index as ScopeId)
            );
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || -(index as i32),
            );
            assert_eq!(
                session.record_node_with_parent(
                    GRANDCHILD_NODE_BASE + index as NodeId,
                    1,
                    Some(CHILD_NODE),
                ),
                NodeSlotUpdate::Reused {
                    id: GRANDCHILD_NODE_BASE + index as NodeId,
                    generation: 1,
                }
            );
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();
    let after = harness.table.debug_stats().mutation;

    let child_index = harness.table.current_group_index(child_anchor);
    let child_range = harness.table.group_subtree_range_at_index(child_index);
    assert_eq!(child_range.root_index(), child_index);
    assert_eq!(child_range.len(), RESTORED_GROUP_COUNT);
    assert_eq!(
        harness.table.groups[child_index..child_index + RESTORED_GROUP_COUNT]
            .iter()
            .map(|group| group.payload_len as usize)
            .sum::<usize>(),
        RESTORED_GROUP_COUNT
    );
    assert_eq!(
        harness.table.groups[child_index..child_index + RESTORED_GROUP_COUNT]
            .iter()
            .map(|group| group.node_len as usize)
            .sum::<usize>(),
        RESTORED_GROUP_COUNT
    );
    assert_eq!(
        harness.table.group_node_record_at(child_index, 0).id,
        CHILD_NODE
    );
    assert_eq!(*harness.table.read_value::<i32>(child_slot), 77);
    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        1
    );
    assert_eq!(
        after.payload_location_range_refresh_payload_count
            - before.payload_location_range_refresh_payload_count,
        RESTORED_GROUP_COUNT
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn perf_mass_conditional_removal_batches_anchor_refresh() {
    const ROOT_KEY: Key = 8_030;
    const PARENT_KEY: Key = 8_031;
    const CHILD_KEY: Key = 8_032;
    const TRAILING_KEY: Key = 8_033;
    const CHILD_COUNT: Key = 64;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in 0..CHILD_COUNT {
            begin_keyed(session, CHILD_KEY, explicit_key, None);
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                || explicit_key as i32,
            );
            let result = session.finish_group_body();
            assert!(result.detached_children.is_empty());
            session.end_group();
        }
        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
        begin_unkeyed(session, TRAILING_KEY, None);
        let trailing_result = session.finish_group_body();
        assert!(trailing_result.detached_children.is_empty());
        session.end_group();
        let root_result = session.finish_group_body();
        assert!(root_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        begin_unkeyed(session, TRAILING_KEY, None);
        let trailing_result = session.finish_group_body();
        assert!(trailing_result.detached_children.is_empty());
        session.end_group();
        let root_result = session.finish_group_body();
        assert!(root_result.detached_children.is_empty());
        session.end_group();
        parent_result.detached_children
    });
    harness.finish_pass();

    assert_eq!(detached.len(), CHILD_COUNT as usize);
    let after = harness.table.debug_stats().mutation;
    assert_eq!(
        after.group_index_refresh_count - before.group_index_refresh_count,
        1
    );
    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        0
    );
}

#[test]
fn perf_repeated_tail_removal_requests_compaction_and_preserves_retained_payloads() {
    const PARENT_KEY: Key = 8_060;
    const RETAINED_KEY: Key = 8_061;
    const TAIL_KEY: Key = 8_062;
    const RETAINED_SCOPE: ScopeId = 8_063;
    const TAIL_PAYLOAD_COUNT: usize = SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD + 16;
    const MID_TAIL_PAYLOAD_COUNT: usize = SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD + 8;
    const KEPT_TAIL_PAYLOAD_COUNT: usize = 8;
    const TAIL_NODE_COUNT: usize = 72;
    const MID_TAIL_NODE_COUNT: usize = 64;
    const KEPT_TAIL_NODE_COUNT: usize = 8;
    const TAIL_NODE_BASE: NodeId = 80_700;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, retained_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);

        let retained = begin_unkeyed(session, RETAINED_KEY, None);
        session.set_group_scope(retained.group, RETAINED_SCOPE);
        let retained_slot = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || 901_i32,
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, TAIL_KEY, None);
        for index in 0..TAIL_PAYLOAD_COUNT {
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || index as i32,
            );
        }
        for index in 0..TAIL_NODE_COUNT {
            session.record_node_with_parent(TAIL_NODE_BASE + index as NodeId, 1, None);
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (parent.anchor, retained_slot)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached_retained = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let tail = begin_unkeyed(session, TAIL_KEY, None);
        assert_eq!(tail.kind, GroupStartKind::Moved);
        for index in 0..MID_TAIL_PAYLOAD_COUNT {
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || index as i32,
            );
        }
        for index in 0..MID_TAIL_NODE_COUNT {
            assert_eq!(
                session.record_node_with_parent(TAIL_NODE_BASE + index as NodeId, 1, None),
                NodeSlotUpdate::Reused {
                    id: TAIL_NODE_BASE + index as NodeId,
                    generation: 1,
                }
            );
        }
        let tail_result = session.finish_group_body();
        assert!(tail_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });

    assert!(!harness.state.request_compaction);
    assert!(!harness.state.request_payload_storage_compaction);
    assert_eq!(
        harness.state.removed_payload_count,
        TAIL_PAYLOAD_COUNT - MID_TAIL_PAYLOAD_COUNT + detached_retained.payload_count()
    );
    assert_eq!(
        harness.state.removed_node_count,
        TAIL_NODE_COUNT - MID_TAIL_NODE_COUNT
    );
    harness.finish_pass();

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached_retained.root_key(),
    };
    let retained_identity = PayloadIdentity::from(retained_slot);
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached_retained);
    assert_eq!(retention.validate(&harness.table), Ok(()));
    assert_eq!(
        harness
            .identity_snapshot(Some(&retention), &[retained_slot])
            .retained_payload_anchors,
        vec![retained_identity]
    );
    assert_eq!(
        harness
            .table
            .payload_anchor_lifecycle(retained_slot.anchor()),
        Some(PayloadAnchorLifecycle::Detached)
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let tail = begin_unkeyed(session, TAIL_KEY, None);
        assert_eq!(tail.kind, GroupStartKind::Reused);
        for index in 0..KEPT_TAIL_PAYLOAD_COUNT {
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || index as i32,
            );
        }
        for index in 0..KEPT_TAIL_NODE_COUNT {
            assert_eq!(
                session.record_node_with_parent(TAIL_NODE_BASE + index as NodeId, 1, None),
                NodeSlotUpdate::Reused {
                    id: TAIL_NODE_BASE + index as NodeId,
                    generation: 1,
                }
            );
        }
        let tail_result = session.finish_group_body();
        assert!(tail_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });

    assert!(harness.state.request_compaction);
    assert!(harness.state.request_payload_storage_compaction);
    assert_eq!(
        harness.state.removed_payload_count,
        MID_TAIL_PAYLOAD_COUNT - KEPT_TAIL_PAYLOAD_COUNT
    );
    assert_eq!(
        harness.state.removed_node_count,
        MID_TAIL_NODE_COUNT - KEPT_TAIL_NODE_COUNT
    );
    harness.finish_pass();

    harness
        .table
        .compact_payload_anchor_registry_storage(Some(&mut retention));
    harness
        .table
        .compact_anchor_registry_storage(Some(&mut retention));
    assert_eq!(retention.validate(&harness.table), Ok(()));
    assert_eq!(
        harness
            .identity_snapshot(Some(&retention), &[retained_slot])
            .retained_payload_anchors,
        vec![retained_identity]
    );
    assert_eq!(
        harness
            .table
            .payload_anchor_lifecycle(retained_slot.anchor()),
        Some(PayloadAnchorLifecycle::Detached)
    );

    let restored = retention
        .take(retain_key)
        .expect("retained payload must survive compaction");
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let retained = begin_unkeyed(session, RETAINED_KEY, Some(restored));
        assert_eq!(retained.kind, GroupStartKind::Restored);
        assert_eq!(retained.scope_id, Some(RETAINED_SCOPE));
        let restored_slot = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || -1_i32,
        );
        assert_eq!(restored_slot, retained_slot);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let tail = begin_unkeyed(session, TAIL_KEY, None);
        assert_eq!(tail.kind, GroupStartKind::Reused);
        for index in 0..KEPT_TAIL_PAYLOAD_COUNT {
            let _ = session.value_slot_with_kind(
                PayloadKind::Internal,
                crate::slot::BRANCH_PATH_ROOT,
                move || index as i32,
            );
        }
        for index in 0..KEPT_TAIL_NODE_COUNT {
            assert_eq!(
                session.record_node_with_parent(TAIL_NODE_BASE + index as NodeId, 1, None),
                NodeSlotUpdate::Reused {
                    id: TAIL_NODE_BASE + index as NodeId,
                    generation: 1,
                }
            );
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(*harness.table.read_value::<i32>(retained_slot), 901);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn perf_storage_compaction_does_not_rebuild_scope_index() {
    const PARENT_KEY: Key = 8_040;
    const CHILD_KEY: Key = 8_041;
    const CHILD_COUNT: usize = 1_100;
    const KEPT_EXPLICIT_KEY: Key = (CHILD_COUNT - 1) as Key;
    const CHILD_SCOPE: ScopeId = 8_042;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let parent_anchor = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        for explicit_key in 0..CHILD_COUNT as Key {
            let child = begin_keyed(session, CHILD_KEY, explicit_key, None);
            if explicit_key == KEPT_EXPLICIT_KEY {
                session.set_group_scope(child.group, CHILD_SCOPE);
                let _ = session.value_slot_with_kind(
                    PayloadKind::Internal,
                    crate::slot::BRANCH_PATH_ROOT,
                    || 1_i32,
                );
            }
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
    let (kept_anchor, detached) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let child = begin_keyed(session, CHILD_KEY, KEPT_EXPLICIT_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _ = session.value_slot_with_kind(
            PayloadKind::Internal,
            crate::slot::BRANCH_PATH_ROOT,
            || 2_i32,
        );
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();
        let parent_result = session.finish_group_body();
        session.end_group();
        (child.anchor, parent_result.detached_children)
    });
    harness.finish_pass();

    for subtree in detached {
        harness.table.invalidate_detached_subtree_anchors(&subtree);
        harness.lifecycle.queue_subtree_disposal(subtree);
    }
    harness.lifecycle.flush_pending_drops();

    assert!(kept_anchor.id as usize > 1_024);
    let before = harness.table.debug_stats().mutation;
    harness.table.compact_anchor_registry_storage(None);
    let after = harness.table.debug_stats().mutation;

    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        0
    );
    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        Some(kept_anchor)
    );
    assert_eq!(harness.table.validate(), Ok(()));
}
