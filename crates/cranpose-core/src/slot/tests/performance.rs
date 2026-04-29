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
            let _ = session.value_slot_with_kind(PayloadKind::Internal, || explicit_key as i32);
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
            let _ = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
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
    assert_eq!(
        after.scope_index_rebuild_count - before.scope_index_rebuild_count,
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
    assert_eq!(
        after_insert.scope_index_rebuild_count - before_insert.scope_index_rebuild_count,
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
    assert_eq!(
        after_remove.scope_index_rebuild_count - before_remove.scope_index_rebuild_count,
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
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || -1_i32);
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
    assert_eq!(
        after.scope_index_rebuild_count - before.scope_index_rebuild_count,
        0
    );
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
            let _ = session.value_slot_with_kind(PayloadKind::Internal, || explicit_key as i32);
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
        after.scope_index_rebuild_count - before.scope_index_rebuild_count,
        0
    );
    assert_eq!(
        after.payload_location_range_refresh_count - before.payload_location_range_refresh_count,
        0
    );
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
                let _ = session.value_slot_with_kind(PayloadKind::Internal, || 1_i32);
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
        let _ = session.value_slot_with_kind(PayloadKind::Internal, || 2_i32);
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
        after.scope_index_rebuild_count - before.scope_index_rebuild_count,
        0
    );
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
