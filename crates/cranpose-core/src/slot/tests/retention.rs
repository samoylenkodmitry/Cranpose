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
fn retained_anchor_restore_reactivates_same_anchor_tree_and_scopes() {
    const PARENT_KEY: Key = 382;
    const CHILD_KEY: Key = 383;
    const GRANDCHILD_KEY: Key = 384;
    const CHILD_SCOPE: ScopeId = 385;
    const GRANDCHILD_SCOPE: ScopeId = 386;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_anchor, grandchild_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);

        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.set_group_scope(grandchild.group, GRANDCHILD_SCOPE);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        (parent.anchor, child.anchor, grandchild.anchor)
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

    assert_eq!(detached.groups[0].anchor, child_anchor);
    assert_eq!(detached.groups[0].parent_anchor, AnchorId::INVALID);
    assert_eq!(detached.groups[1].anchor, grandchild_anchor);
    assert_eq!(detached.groups[1].parent_anchor, child_anchor);

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness.begin_pass(SlotPassMode::Compose);
    let (child_kind, child_scope, grandchild_kind, grandchild_scope) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        let child = begin_unkeyed(session, CHILD_KEY, Some(restored));
        assert_eq!(child.anchor, child_anchor);

        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        assert_eq!(grandchild.anchor, grandchild_anchor);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();

        (
            child.kind,
            child.scope_id,
            grandchild.kind,
            grandchild.scope_id,
        )
    });
    harness.finish_pass();

    assert_eq!(child_kind, GroupStartKind::Restored);
    assert_eq!(child_scope, Some(CHILD_SCOPE));
    assert_eq!(grandchild_kind, GroupStartKind::Reused);
    assert_eq!(grandchild_scope, Some(GRANDCHILD_SCOPE));
    assert_eq!(harness.table.groups[1].parent_anchor, parent_anchor);
    assert_eq!(harness.table.groups[2].parent_anchor, child_anchor);
    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        Some(child_anchor)
    );
    assert_eq!(
        harness.table.scope_index_anchor(GRANDCHILD_SCOPE),
        Some(grandchild_anchor)
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn scope_index_lifecycle_survives_retain_restore_dispose_and_compaction() {
    const PARENT_KEY: Key = 390;
    const CHILD_STATIC_KEY: Key = 391;
    const CHILD_COUNT: usize = 1_100;
    const RETAINED_EXPLICIT_KEY: Key = (CHILD_COUNT - 1) as Key;
    const CHILD_SCOPE: ScopeId = 392;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, retained_anchor) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        let mut retained_anchor = None;
        for explicit_key in 0..CHILD_COUNT as Key {
            let child = begin_keyed(session, CHILD_STATIC_KEY, explicit_key, None);
            if explicit_key == RETAINED_EXPLICIT_KEY {
                session.set_group_scope(child.group, CHILD_SCOPE);
                retained_anchor = Some(child.anchor);
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
            retained_anchor.expect("retained child anchor must be captured"),
        )
    });
    harness.finish_pass();

    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        Some(retained_anchor),
        "active scoped child must be indexed by scope id",
    );
    assert!(
        retained_anchor.id as usize > 1_024,
        "test must exercise sparse retained scope-anchor storage"
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

    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        None,
        "detached scopes must leave the active scope index",
    );

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

    let retained = retained.expect("target retained subtree must detach");
    assert_eq!(retained.groups[0].anchor, retained_anchor);
    let retain_key = RetainKey {
        parent_scope: None,
        key: retained.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, retained);
    assert_eq!(retention.validate(&harness.table), Ok(()));

    let snapshot_before = harness.identity_snapshot(Some(&retention), &[]);
    assert_eq!(
        snapshot_before.retained_group_anchors,
        vec![retained_anchor]
    );
    assert!(snapshot_before.scope_ids.contains(&CHILD_SCOPE));

    harness
        .table
        .compact_anchor_registry_storage(Some(&mut retention));
    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        None,
        "retained scopes must stay out of the active scope index after compaction",
    );
    assert_eq!(
        harness
            .identity_snapshot(Some(&retention), &[])
            .retained_group_anchors,
        snapshot_before.retained_group_anchors,
        "retained scope compaction must not rename group anchors",
    );

    let restored = retention
        .take(retain_key)
        .expect("retained subtree must restore");
    harness.begin_pass(SlotPassMode::Compose);
    let (restored_kind, restored_anchor, restored_scope) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let child = begin_keyed(
            session,
            CHILD_STATIC_KEY,
            RETAINED_EXPLICIT_KEY,
            Some(restored),
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (child.kind, child.anchor, child.scope_id)
    });
    harness.finish_pass();

    assert_eq!(restored_kind, GroupStartKind::Restored);
    assert_eq!(restored_anchor, retained_anchor);
    assert_eq!(restored_scope, Some(CHILD_SCOPE));
    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        Some(retained_anchor),
        "restored scopes must re-enter the active scope index",
    );

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        let group = session
            .begin_recompose_at_scope(CHILD_SCOPE)
            .expect("restored scope must resolve through the active scope index");
        assert_eq!(session.table.active_group_anchor(group), retained_anchor);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_recompose();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let disposed = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);
        let result = session.finish_group_body();
        session.end_group();
        assert_eq!(result.detached_children.len(), 1);
        result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    harness.table.invalidate_detached_subtree_anchors(&disposed);
    harness.lifecycle.queue_subtree_disposal(disposed);
    harness.lifecycle.flush_pending_drops();

    assert_eq!(
        harness.table.scope_index_anchor(CHILD_SCOPE),
        None,
        "disposed scopes must not remain indexed",
    );
    assert_eq!(
        harness.table.anchor_state(retained_anchor),
        None,
        "disposed scope anchors must be invalidated"
    );
    assert_eq!(harness.table.validate(), Ok(()));
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
        .scope_index
        .insert_for_test(CHILD_SCOPE, retained_anchor);

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
fn finalize_pass_returns_root_level_detached_subtree_for_caller_cleanup() {
    const ROOT_KEY: Key = 358;
    const ROOT_NODE: NodeId = 359;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let root_slot = harness.session(|session| {
        begin_unkeyed(session, ROOT_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 73_i32);
        session.record_node_with_parent(ROOT_NODE, 1, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let detached_root_children = {
        let mut session = harness
            .table
            .write_session(&mut harness.lifecycle, &mut harness.state);
        session.finalize_pass(&mut harness.applier)
    };

    assert_eq!(detached_root_children.len(), 1);
    let subtree = detached_root_children.into_iter().next().unwrap();
    assert_eq!(subtree.root_key().static_key, ROOT_KEY);
    assert_eq!(subtree.payload_count(), 1);
    assert_eq!(subtree.node_ids_iter().collect::<Vec<_>>(), vec![ROOT_NODE]);
    assert!(harness.table.groups.is_empty());

    harness.table.invalidate_detached_subtree_anchors(&subtree);
    harness.lifecycle.queue_subtree_disposal(subtree);
    harness.lifecycle.flush_pending_drops();
    assert_eq!(harness.table.validate(), Ok(()));
    let removed_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(root_slot);
    }));
    assert!(
        removed_read.is_err(),
        "root-level finalization cleanup must invalidate removed payload handles"
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
        assert_eq!(
            recorded,
            NodeSlotUpdate::Reused {
                id: child_id,
                generation: child_generation,
            },
        );
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
