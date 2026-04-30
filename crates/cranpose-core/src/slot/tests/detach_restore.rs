use super::*;

#[test]
fn detach_restore_preserves_nested_payloads_and_scopes() {
    const PARENT_KEY: Key = 100;
    const CHILD_KEY: Key = 200;
    const GRANDCHILD_KEY: Key = 300;
    const CHILD_SCOPE: ScopeId = 7;
    const GRANDCHILD_SCOPE: ScopeId = 8;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (child_slot, grandchild_slot, child_anchor) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let child_slot = session.value_slot_with_kind(PayloadKind::Internal, || 70_i32);
        session.record_node_with_parent(21, 1, None);

        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        session.set_group_scope(grandchild.group, GRANDCHILD_SCOPE);
        let grandchild_slot = session.value_slot_with_kind(PayloadKind::Internal, || 110_i32);
        session.record_node_with_parent(22, 1, None);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (child_slot, grandchild_slot, child.anchor)
    });
    harness.finish_pass();
    harness.table.write_value(child_slot, 71_i32);
    harness.table.write_value(grandchild_slot, 111_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);
        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();
    assert!(
        !anchor_is_active(&harness.table, child_anchor),
        "detached child must leave the active anchor index"
    );
    assert_eq!(
        harness.table.anchors.state(child_anchor),
        Some(AnchorState::Detached),
        "detached child must remain addressable as detached until restored or disposed"
    );
    let detached_child_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(child_slot);
    }));
    assert!(
        detached_child_read.is_err(),
        "detached child payload anchors must stop resolving while inactive"
    );
    let detached_grandchild_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(grandchild_slot);
    }));
    assert!(
        detached_grandchild_read.is_err(),
        "detached nested payload anchors must stop resolving while inactive"
    );
    assert_eq!(detached.groups[0].parent_anchor, AnchorId::INVALID);
    assert_eq!(detached.groups[0].depth, 0);
    assert_eq!(detached.groups[1].parent_anchor, detached.groups[0].anchor);
    assert_eq!(detached.groups[1].depth, 1);

    harness.begin_pass(SlotPassMode::Compose);
    let (
        child_kind,
        restored_child_scope,
        restored_child_slot,
        grandchild_kind,
        restored_grandchild_scope,
        restored_grandchild_slot,
    ) = harness.session(move |session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, Some(detached));
        let child_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);

        let grandchild = begin_unkeyed(session, GRANDCHILD_KEY, None);
        let grandchild_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let grandchild_result = session.finish_group_body();
        assert!(grandchild_result.detached_children.is_empty());
        session.end_group();

        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (
            child.kind,
            child.scope_id,
            child_slot,
            grandchild.kind,
            grandchild.scope_id,
            grandchild_slot,
        )
    });
    harness.finish_pass();

    assert_eq!(child_kind, GroupStartKind::Restored);
    assert_eq!(restored_child_scope, Some(CHILD_SCOPE));
    assert_eq!(grandchild_kind, GroupStartKind::Reused);
    assert_eq!(restored_grandchild_scope, Some(GRANDCHILD_SCOPE));
    assert_eq!(restored_child_slot, child_slot);
    assert_eq!(restored_grandchild_slot, grandchild_slot);
    assert_eq!(*harness.table.read_value::<i32>(child_slot), 71);
    assert_eq!(*harness.table.read_value::<i32>(grandchild_slot), 111);
    assert_eq!(*harness.table.read_value::<i32>(restored_child_slot), 71);
    assert_eq!(
        *harness.table.read_value::<i32>(restored_grandchild_slot),
        111
    );
}

#[test]
fn restore_subtree_between_existing_siblings_reactivates_scope_and_anchor_indexes() {
    const PARENT_KEY: Key = 320;
    const CHILD_A_KEY: Key = 321;
    const CHILD_B_KEY: Key = 322;
    const CHILD_C_KEY: Key = 323;
    const CHILD_B_SCOPE: ScopeId = 324;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_a_anchor, child_b_anchor, child_c_anchor, child_b_slot) = harness
        .session(|session| {
            let parent = begin_unkeyed(session, PARENT_KEY, None);

            let child_a = begin_unkeyed(session, CHILD_A_KEY, None);
            let child_a_result = session.finish_group_body();
            assert!(child_a_result.detached_children.is_empty());
            session.end_group();

            let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
            session.set_group_scope(child_b.group, CHILD_B_SCOPE);
            let child_b_slot = session.value_slot_with_kind(PayloadKind::Internal, || 80_i32);
            let child_b_result = session.finish_group_body();
            assert!(child_b_result.detached_children.is_empty());
            session.end_group();

            let child_c = begin_unkeyed(session, CHILD_C_KEY, None);
            let child_c_result = session.finish_group_body();
            assert!(child_c_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (
                parent.anchor,
                child_a.anchor,
                child_b.anchor,
                child_c.anchor,
                child_b_slot,
            )
        });
    harness.finish_pass();
    harness.table.write_value(child_b_slot, 88_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_A_KEY, None);
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_C_KEY, None);
        let child_c_result = session.finish_group_body();
        assert!(child_c_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    assert_eq!(
        harness.table.anchors.state(child_b_anchor),
        Some(AnchorState::Detached)
    );
    assert!(
        harness
            .table
            .active_group_for_scope(CHILD_B_SCOPE)
            .is_none(),
        "detached scopes must leave the active scope index"
    );

    let insert_index = harness.table.current_group_index(child_c_anchor);
    let restored_anchor = restore_detached_child(
        &mut harness.table,
        parent_anchor,
        insert_index,
        detached.root_key(),
        detached,
    );

    assert_eq!(restored_anchor, child_b_anchor);
    assert_eq!(
        harness.table.anchors.state(restored_anchor),
        Some(AnchorState::Active(2))
    );
    assert_eq!(harness.table.groups[1].anchor, child_a_anchor);
    assert_eq!(harness.table.groups[2].anchor, child_b_anchor);
    assert_eq!(harness.table.groups[3].anchor, child_c_anchor);

    let restored_group = harness
        .table
        .active_group_for_scope(CHILD_B_SCOPE)
        .expect("restored child scope must become active again");
    assert_eq!(
        harness.table.active_group_anchor(restored_group),
        child_b_anchor
    );
    assert_eq!(*harness.table.read_value::<i32>(child_b_slot), 88);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn middle_subtree_detach_and_retained_restore_preserve_payloads_nodes_and_scopes() {
    const PARENT_KEY: Key = 340;
    const CHILD_A_KEY: Key = 341;
    const CHILD_B_KEY: Key = 342;
    const CHILD_C_KEY: Key = 343;
    const CHILD_B_SCOPE: ScopeId = 344;
    const CHILD_B_NODE: NodeId = 345;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (parent_anchor, child_b_anchor, child_b_slot) = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, CHILD_A_KEY, None);
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        let child_b = begin_unkeyed(session, CHILD_B_KEY, None);
        session.set_group_scope(child_b.group, CHILD_B_SCOPE);
        let child_b_slot = session.value_slot_with_kind(PayloadKind::Internal, || 64_i32);
        session.record_node_with_parent(CHILD_B_NODE, 1, None);
        let child_b_result = session.finish_group_body();
        assert!(child_b_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_C_KEY, None);
        let child_c_result = session.finish_group_body();
        assert!(child_c_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (parent.anchor, child_b.anchor, child_b_slot)
    });
    harness.finish_pass();
    harness.table.write_value(child_b_slot, 128_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let detached = harness.session(|session| {
        let parent = begin_unkeyed(session, PARENT_KEY, None);
        assert_eq!(parent.anchor, parent_anchor);

        begin_unkeyed(session, CHILD_A_KEY, None);
        let child_a_result = session.finish_group_body();
        assert!(child_a_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, CHILD_C_KEY, None);
        let child_c_result = session.finish_group_body();
        assert!(child_c_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        session.end_group();
        assert_eq!(parent_result.detached_children.len(), 1);
        parent_result.detached_children.into_iter().next().unwrap()
    });
    harness.finish_pass();

    assert_eq!(detached.root_key().static_key, CHILD_B_KEY);
    assert_eq!(
        detached.group_anchors().collect::<Vec<_>>(),
        vec![child_b_anchor]
    );
    assert_eq!(detached.payload_count(), 1);
    assert_eq!(
        detached.node_ids_iter().collect::<Vec<_>>(),
        vec![CHILD_B_NODE]
    );
    assert_eq!(detached.scope_ids(), vec![CHILD_B_SCOPE]);
    assert_eq!(harness.table.groups[1].key.static_key, CHILD_A_KEY);
    assert_eq!(harness.table.groups[2].key.static_key, CHILD_C_KEY);

    let retain_key = RetainKey {
        parent_scope: None,
        key: detached.root_key(),
    };
    let mut retention = RetentionManager::default();
    retention.insert(retain_key, detached);
    let restored = retention
        .take(retain_key)
        .expect("retained middle subtree must restore");

    harness.begin_pass(SlotPassMode::Compose);
    let (restored_kind, restored_scope, restored_slot, restored_node) =
        harness.session(|session| {
            let parent = begin_unkeyed(session, PARENT_KEY, None);
            assert_eq!(parent.anchor, parent_anchor);

            begin_unkeyed(session, CHILD_A_KEY, None);
            let child_a_result = session.finish_group_body();
            assert!(child_a_result.detached_children.is_empty());
            session.end_group();

            let child_b = begin_unkeyed(session, CHILD_B_KEY, Some(restored));
            let restored_slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
            let restored_node = session.current_node_record();
            let recorded = session.record_node_with_parent(CHILD_B_NODE, 1, None);
            assert_eq!(
                recorded,
                NodeSlotUpdate::Reused {
                    id: CHILD_B_NODE,
                    generation: 1,
                },
            );
            let child_b_result = session.finish_group_body();
            assert!(child_b_result.detached_children.is_empty());
            session.end_group();

            begin_unkeyed(session, CHILD_C_KEY, None);
            let child_c_result = session.finish_group_body();
            assert!(child_c_result.detached_children.is_empty());
            session.end_group();

            let parent_result = session.finish_group_body();
            assert!(parent_result.detached_children.is_empty());
            session.end_group();

            (child_b.kind, child_b.scope_id, restored_slot, restored_node)
        });
    harness.finish_pass();

    assert_eq!(restored_kind, GroupStartKind::Restored);
    assert_eq!(restored_scope, Some(CHILD_B_SCOPE));
    assert_eq!(restored_slot, child_b_slot);
    assert_eq!(restored_node, Some((CHILD_B_NODE, 1)));
    assert_eq!(*harness.table.read_value::<i32>(child_b_slot), 128);
    assert_eq!(
        harness.table.scope_index_anchor(CHILD_B_SCOPE),
        Some(child_b_anchor)
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn restore_subtree_rejects_mismatched_root_key_without_mutating_table() {
    const PARENT_KEY: Key = 354;
    const CHILD_KEY: Key = 355;

    let (mut harness, detached) = detached_single_child(PARENT_KEY, CHILD_KEY);
    let parent_anchor = harness.table.groups[0].anchor;
    let group_count_before = harness.table.groups.len();
    let detached_anchor = detached
        .group_anchors()
        .next()
        .expect("detached subtree must contain an anchor");
    let wrong_key = GroupKey::new(CHILD_KEY + 1, None, 0);

    let restore = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        restore_detached_child(&mut harness.table, parent_anchor, 1, wrong_key, detached);
    }));
    assert!(
        restore.is_err(),
        "restoring a detached subtree under a different group key must be rejected",
    );
    assert_eq!(harness.table.groups.len(), group_count_before);
    assert_eq!(
        harness.table.anchor_state(detached_anchor),
        Some(AnchorState::Detached),
        "failed restore must leave the detached anchor out of the active table",
    );
    harness.table.debug_verify();
}

#[test]
fn removing_conditional_child_returns_detached_subtree() {
    const PARENT_KEY: Key = 350;
    const CHILD_KEY: Key = 351;
    const CHILD_SCOPE: ScopeId = 91;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 17_i32);
        session.record_node_with_parent(41, 1, None);
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

    assert_eq!(detached.root_key().static_key, CHILD_KEY);
    assert_eq!(detached.root_scope_id(), Some(CHILD_SCOPE));
    assert_eq!(detached.group_count(), 1);
    assert_eq!(detached.node_count(), 1);
    assert_eq!(detached.node_ids_iter().collect::<Vec<_>>(), vec![41]);
    let mut root_nodes = Vec::new();
    detached.collect_root_nodes_into(&mut root_nodes);
    assert_eq!(root_nodes, vec![41]);
    assert_eq!(
        detached.node_states().collect::<Vec<_>>(),
        vec![(41, super::NodeLifecycle::Active)]
    );
    assert_eq!(detached.scope_ids(), vec![CHILD_SCOPE]);
    assert_eq!(detached.group_anchors().collect::<Vec<_>>().len(), 1);
    assert_eq!(detached.groups[0].parent_anchor, AnchorId::INVALID);
    assert_eq!(detached.groups[0].depth, 0);
}
