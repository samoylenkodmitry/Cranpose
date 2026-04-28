use super::*;

#[test]
fn identity_snapshot_captures_active_and_retained_identities() {
    const PARENT_KEY: Key = 362;
    const CHILD_KEY: Key = 363;
    const CHILD_SCOPE: ScopeId = 21;

    let mut harness = SlotHarness::new();
    let mut child_slot = None;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        child_slot = Some(session.value_slot_with_kind(PayloadKind::Internal, || 17_i32));
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let child_slot = child_slot.expect("child payload slot must be captured");
    let active_snapshot = harness.identity_snapshot(None, &[child_slot]);
    assert_eq!(active_snapshot.value_slots, vec![child_slot]);
    assert_eq!(active_snapshot.active_group_anchors.len(), 2);
    assert!(active_snapshot.retained_group_anchors.is_empty());
    assert_eq!(
        active_snapshot.active_payload_anchors,
        vec![PayloadIdentity::from(child_slot)]
    );
    assert!(active_snapshot.scope_ids.contains(&CHILD_SCOPE));
    assert_eq!(active_snapshot.debug_stats.group_count, 2);
    assert_eq!(active_snapshot.debug_stats.payload_count, 1);
    assert_eq!(active_snapshot.debug_stats.retained_payload_count, 0);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    let retained_group_anchors = detached
        .groups
        .iter()
        .map(|group| group.anchor)
        .collect::<Vec<_>>();
    let retained_payload_anchors = detached
        .payloads
        .iter()
        .map(PayloadIdentity::from)
        .collect::<Vec<_>>();
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);

    let retained_snapshot = harness.identity_snapshot(Some(&retention), &[child_slot]);
    assert_eq!(retained_snapshot.value_slots, vec![child_slot]);
    assert_eq!(
        retained_snapshot.retained_group_anchors,
        retained_group_anchors
    );
    assert_eq!(
        retained_snapshot.retained_payload_anchors,
        retained_payload_anchors
    );
    assert_eq!(
        retained_snapshot.retained_payload_anchors,
        vec![PayloadIdentity::from(child_slot)]
    );
    assert!(retained_snapshot.scope_ids.contains(&CHILD_SCOPE));
    assert_eq!(retained_snapshot.debug_stats.retained_group_count, 1);
    assert_eq!(retained_snapshot.debug_stats.retained_payload_count, 1);
    assert_eq!(retained_snapshot.debug_stats.retained_scope_count, 1);
}

#[test]
fn retention_marks_detached_nodes_and_reactivates_on_take() {
    const PARENT_KEY: Key = 364;
    const CHILD_KEY: Key = 365;

    let mut harness = SlotHarness::new();
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::new(Cell::new(0)))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(child_id, child_generation, None);
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

    let key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(key, detached);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let restored = retention.take(key).expect("retained subtree must exist");
    assert_eq!(
        restored.node_states().collect::<Vec<_>>(),
        vec![(child_id, super::NodeLifecycle::Active)]
    );
}

#[test]
fn retention_insert_rejects_duplicate_key_without_replacing_existing_subtree() {
    const PARENT_KEY: Key = 366;
    const CHILD_KEY: Key = 367;

    let (harness, first_detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let (_, duplicate_detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let retain_key = RetainKey {
        parent_scope: None,
        key: first_detached.root_key(),
    };
    assert_eq!(duplicate_detached.root_key(), retain_key.key);

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, first_detached);

    let duplicate_insert = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        retention.insert(retain_key, duplicate_detached);
    }));
    assert!(
        duplicate_insert.is_err(),
        "retention must reject duplicate retained keys instead of dropping the existing subtree",
    );
    assert_eq!(retention.debug_stats().subtree_count, 1);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let restored = retention
        .take(retain_key)
        .expect("original retained subtree must remain available after duplicate rejection");
    assert_eq!(restored.root_key(), retain_key.key);
}

#[test]
fn retention_validate_rejects_root_key_mismatch() {
    const PARENT_KEY: Key = 370;
    const CHILD_KEY: Key = 371;

    let (harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let actual_root_key = detached.root_key();
    let retain_key = RetainKey {
        parent_scope: None,
        key: GroupKey::new(CHILD_KEY + 10, None, 0),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedRootKeyMismatch {
            parent_scope: None,
            expected: retain_key.key,
            actual: actual_root_key,
        })
    );
}

#[test]
fn retention_validate_rejects_active_retained_anchor() {
    const PARENT_KEY: Key = 372;
    const CHILD_KEY: Key = 373;

    let (mut harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let retained_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    harness.table.anchors.set_active(retained_anchor, 0);

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedSubtreeAnchorStillActive {
            root_key: retain_key.key,
            anchor: retained_anchor,
            active_index: 0,
        })
    );
}

#[test]
fn retention_validate_rejects_non_detached_retained_anchor() {
    const PARENT_KEY: Key = 374;
    const CHILD_KEY: Key = 375;

    let (mut harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let retained_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    harness.table.anchors.clear();

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedAnchorStateMismatch {
            root_key: retain_key.key,
            anchor: retained_anchor,
            actual: None,
        })
    );
}

#[test]
fn retention_validate_rejects_active_retained_payload_anchor() {
    const PARENT_KEY: Key = 378;
    const CHILD_KEY: Key = 379;

    let (mut harness, detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, true, false);
    let payload = detached
        .payloads
        .first()
        .expect("detached subtree must contain a payload");
    let retained_payload_anchor = payload.anchor;
    let retained_payload_owner = payload.owner;
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    harness
        .table
        .payload_anchors
        .set_active(retained_payload_anchor, retained_payload_owner, 0);

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedPayloadAnchorStillActive {
            root_key: retain_key.key,
            payload_anchor: retained_payload_anchor,
            active_owner: retained_payload_owner,
            active_index: 0,
        })
    );
}

#[test]
fn retention_validate_rejects_non_detached_retained_payload_anchor() {
    const PARENT_KEY: Key = 380;
    const CHILD_KEY: Key = 381;

    let (mut harness, detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, true, false);
    let retained_payload_anchor = detached
        .payloads
        .first()
        .expect("detached subtree must contain a payload")
        .anchor;
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    harness.table.payload_anchors.clear();

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedPayloadAnchorStateMismatch {
            root_key: retain_key.key,
            payload_anchor: retained_payload_anchor,
            actual: None,
        })
    );
}

#[test]
fn retention_validate_rejects_retained_scope_in_active_scope_index() {
    const PARENT_KEY: Key = 376;
    const CHILD_KEY: Key = 377;
    const CHILD_SCOPE: ScopeId = 23;

    let (mut harness, detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, Some(CHILD_SCOPE), false, false);
    let retained_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    harness
        .table
        .scope_anchor_to_group
        .insert(CHILD_SCOPE, retained_anchor);

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedScopeStillActive {
            root_key: retain_key.key,
            scope_id: CHILD_SCOPE,
            active_anchor: retained_anchor,
        })
    );
}

#[test]
fn retention_validate_rejects_active_retained_node() {
    const PARENT_KEY: Key = 378;
    const CHILD_KEY: Key = 379;

    let (harness, detached, child_node) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, false, true);
    let child_node = child_node.expect("test helper must record a child node");
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    retention
        .subtrees_mut()
        .next()
        .expect("retained subtree must exist")
        .mark_nodes_active();

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedNodeLifecycleMismatch {
            root_key: retain_key.key,
            node_id: child_node,
            actual: super::NodeLifecycle::Active,
        })
    );
}

#[test]
fn retention_validate_rejects_retained_root_with_active_parent() {
    const PARENT_KEY: Key = 380;
    const CHILD_KEY: Key = 381;

    let (harness, mut detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let parent_anchor = harness.table.groups[0].anchor;
    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    detached.groups[0].parent_anchor = parent_anchor;

    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);

    assert_eq!(
        retention.validate(&harness.table),
        Err(SlotInvariantError::RetainedRootHasActiveParent {
            root_key: retain_key.key,
            parent_anchor,
        })
    );
}

#[test]
fn detached_validate_rejects_non_preorder_parent() {
    const PARENT_KEY: Key = 386;
    const CHILD_KEY: Key = 387;
    const GRANDCHILD_KEY: Key = 388;

    let (_, mut detached) = detached_child_with_grandchild(PARENT_KEY, CHILD_KEY, GRANDCHILD_KEY);
    let root_key = detached.root_key();
    let expected_parent = detached.groups[0].anchor;
    detached.groups[1].parent_anchor = AnchorId::INVALID;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::InvalidParent {
            tree: SlotTreeContext::Detached { root_key },
            group_index: 1,
            expected: expected_parent,
            actual: AnchorId::INVALID,
        })
    );
}

#[test]
fn detached_validate_rejects_bad_depth() {
    const PARENT_KEY: Key = 389;
    const CHILD_KEY: Key = 390;
    const GRANDCHILD_KEY: Key = 391;

    let (_, mut detached) = detached_child_with_grandchild(PARENT_KEY, CHILD_KEY, GRANDCHILD_KEY);
    let root_key = detached.root_key();
    detached.groups[1].depth = 2;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::BadDepth {
            tree: SlotTreeContext::Detached { root_key },
            group_index: 1,
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn detached_validate_rejects_subtree_len_out_of_range() {
    const PARENT_KEY: Key = 392;
    const CHILD_KEY: Key = 393;
    const GRANDCHILD_KEY: Key = 394;

    let (_, mut detached) = detached_child_with_grandchild(PARENT_KEY, CHILD_KEY, GRANDCHILD_KEY);
    let root_key = detached.root_key();
    detached.groups[0].subtree_len = 3;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::BadSubtreeLen {
            tree: SlotTreeContext::Detached { root_key },
            group_index: 0,
            expected: 0,
            actual: 3,
        })
    );
}

#[test]
fn detached_validate_rejects_payload_owner_outside_subtree() {
    const PARENT_KEY: Key = 395;
    const CHILD_KEY: Key = 396;

    let (harness, mut detached, _) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, true, false);
    let root_key = detached.root_key();
    let expected_owner = detached.groups[0].anchor;
    let outside_anchor = harness.table.groups[0].anchor;
    let payload_anchor = detached.payloads[0].anchor;
    detached.payloads[0].owner = outside_anchor;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::PayloadOwnerMismatch {
            tree: SlotTreeContext::Detached { root_key },
            payload_anchor: payload_anchor.id(),
            expected: expected_owner,
            actual: outside_anchor,
        })
    );
}

#[test]
fn detached_validate_rejects_node_owner_outside_subtree() {
    const PARENT_KEY: Key = 397;
    const CHILD_KEY: Key = 398;

    let (harness, mut detached, child_node) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, false, true);
    let root_key = detached.root_key();
    let expected_owner = detached.groups[0].anchor;
    let outside_anchor = harness.table.groups[0].anchor;
    let child_node = child_node.expect("test helper must record a child node");
    detached.nodes[0].owner = outside_anchor;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::NodeOwnerMismatch {
            tree: SlotTreeContext::Detached { root_key },
            node_id: child_node,
            expected: expected_owner,
            actual: outside_anchor,
        })
    );
}

#[test]
fn detached_validate_rejects_duplicate_node_id() {
    const PARENT_KEY: Key = 399;
    const CHILD_KEY: Key = 400;

    let (_, mut detached, child_node) =
        detached_single_child_with_options(PARENT_KEY, CHILD_KEY, None, false, true);
    let root_key = detached.root_key();
    let child_node = child_node.expect("test helper must record a child node");
    detached.nodes.push(detached.nodes[0]);
    detached.groups[0].node_len = 2;
    detached.groups[0].subtree_node_count = 2;

    assert_eq!(
        detached.validate_detached(),
        Err(SlotInvariantError::DuplicateNodeId {
            tree: SlotTreeContext::Detached { root_key },
            node_id: child_node,
        })
    );
}

#[test]
fn retention_debug_stats_report_retained_payload_anchor_and_heap_counts() {
    const PARENT_KEY: Key = 368;
    const CHILD_KEY: Key = 369;
    const CHILD_SCOPE: ScopeId = 22;

    let mut harness = SlotHarness::new();
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::new(Cell::new(0)))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _remembered = session.remember(|| 91_i32);
        session.record_node_with_parent(child_id, child_generation, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 0);
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
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    let stats = retention.debug_stats();

    assert_eq!(stats.subtree_count, 1);
    assert_eq!(stats.group_count, 1);
    assert_eq!(stats.payload_count, 1);
    assert_eq!(stats.node_count, 1);
    assert_eq!(stats.scope_count, 1);
    assert_eq!(stats.anchor_count, 1);
    assert!(stats.heap_bytes > 0);
    assert_eq!(stats.evictions_total, 0);
}

#[test]
fn finalize_pass_disposes_removed_child_nodes() {
    const PARENT_KEY: Key = 360;
    const CHILD_KEY: Key = 361;

    let mut harness = SlotHarness::new();
    let child_unmounts = Rc::new(Cell::new(0));
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::clone(
            &child_unmounts,
        ))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(child_id, child_generation, None);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "active child nodes must remain live before removal"
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
    });
    harness.finish_pass();

    assert_eq!(child_unmounts.get(), 1);
    assert_eq!(harness.applier.len(), 0);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_err(),
        "disposed child nodes must be physically removed from the applier"
    );
}

#[test]
fn retained_detached_child_nodes_stay_live_across_restore() {
    const PARENT_KEY: Key = 362;
    const CHILD_KEY: Key = 363;

    let mut harness = SlotHarness::new();
    let child_unmounts = Rc::new(Cell::new(0));
    let child_id = harness
        .applier
        .create(Box::new(UnmountTrackingNode::new(Rc::clone(
            &child_unmounts,
        ))));
    let child_generation = harness.applier.node_generation(child_id);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_KEY, None);
        session.record_node_with_parent(child_id, child_generation, None);
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

    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![child_id]);
    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "retained detached nodes must remain live while held by the caller"
    );

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(move |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, Some(detached));
        assert_eq!(child.kind, GroupStartKind::Restored);
        assert_eq!(
            session.current_node_record(),
            Some((child_id, child_generation)),
            "restored children must expose their retained node for explicit reuse"
        );
        let recorded = session.record_node_with_parent(child_id, child_generation, None);
        assert!(recorded.reused);
        assert_eq!(recorded.id, child_id);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(child_unmounts.get(), 0);
    assert_eq!(harness.applier.len(), 1);
    assert!(
        harness
            .applier
            .with_node::<UnmountTrackingNode, _>(child_id, |_| ())
            .is_ok(),
        "restored retained nodes must stay live instead of being recreated or disposed"
    );
}
