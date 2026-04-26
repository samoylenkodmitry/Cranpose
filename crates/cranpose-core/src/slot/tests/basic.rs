use super::*;

#[test]
fn slot_v2_empty_table_validates() {
    let table = SlotTable::new();
    assert_eq!(table.validate(), Ok(()));
    assert!(table.groups.is_empty());
    assert!(table.debug_dump_groups().is_empty());
    assert!(table.debug_dump_slot_entries().is_empty());
}

#[test]
fn first_composition_records_group_value_and_node() {
    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        let started = begin_unkeyed(session, 1, None);
        assert_eq!(started.kind, GroupStartKind::Inserted);
        let slot = session.value_slot(|| 41_i32);
        session.record_node(7, 1);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert!(result.direct_nodes.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(*harness.table.read_value::<i32>(slot), 41);
    assert_eq!(harness.table.groups.len(), 1);
    assert_eq!(harness.table.total_payload_count(), 1);
    assert_eq!(harness.table.groups[0].payload_len, 1);
    assert_eq!(harness.table.total_node_count(), 1);
    assert_eq!(harness.table.groups[0].node_len, 1);
    assert_eq!(harness.table.groups[0].subtree_len, 1);
    assert_eq!(harness.table.groups[0].subtree_node_count, 1);
    let payload = harness.table.group_payload_record_at(0, 0);
    assert_eq!(payload.type_id, TypeId::of::<i32>());
    assert_eq!(
        harness.table.group_node_record_at(0, 0).lifecycle,
        super::NodeLifecycle::Active
    );
}

#[test]
fn removing_many_payloads_requests_compaction() {
    const GROUP_KEY: Key = 78;
    const PAYLOAD_COUNT: usize = SlotWriteSessionState::COMPACT_PAYLOAD_THRESHOLD;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(started.kind, GroupStartKind::Inserted);
        for index in 0..PAYLOAD_COUNT {
            let _ = session.value_slot(move || index as i32);
        }
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert!(!harness.state.request_compaction);
    assert_eq!(harness.table.total_payload_count(), PAYLOAD_COUNT);

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let started = begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(started.kind, GroupStartKind::Reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });

    assert_eq!(harness.state.removed_payload_count, PAYLOAD_COUNT);
    assert!(harness.state.request_compaction);
    harness.finish_pass();
}

#[test]
fn slot_write_session_exposes_semantic_operations() {
    const GROUP_KEY: Key = 77;
    const SCOPE_ID: ScopeId = 901;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let (composed_group, slot) = harness.session(|session| {
        exercise_slot_write_session_surface(session, GroupKey::new(GROUP_KEY, None, 0), SCOPE_ID)
    });
    harness.finish_pass();
    assert_eq!(*harness.table.read_value::<i32>(slot), 7);
    *harness.table.read_value_mut::<i32>(slot) = 8;
    harness.table.write_value(slot, 9_i32);
    assert_eq!(*harness.table.read_value::<i32>(slot), 9);
    assert_eq!(harness.table.validate(), Ok(()));
    assert_eq!(harness.table.debug_snapshot().active_groups.len(), 1);

    harness.begin_pass(SlotPassMode::Recompose);
    harness.session(|session| {
        let recomposed_group = session
            .begin_recompose_at_scope(SCOPE_ID)
            .expect("session should resolve the indexed scope");
        assert_eq!(recomposed_group, composed_group);
        assert_eq!(session.nodes_in_current_group(), vec![55]);
        session.skip_group();
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert_eq!(result.root_nodes, vec![55]);
        assert!(result.was_skipped);
        session.end_recompose();
    });
    harness.finish_pass();
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn debug_snapshot_reports_active_groups_anchors_and_scopes() {
    const ROOT_KEY: Key = 41;
    const CHILD_KEY: Key = 42;
    const ROOT_SCOPE: ScopeId = 101;
    const CHILD_SCOPE: ScopeId = 202;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        let root = begin_unkeyed(session, ROOT_KEY, None);
        session.set_group_scope(root.group, ROOT_SCOPE);
        let _ = session.value_slot(|| 7_i32);
        session.record_node(11, 1);

        let child = begin_unkeyed(session, CHILD_KEY, None);
        session.set_group_scope(child.group, CHILD_SCOPE);
        let _ = session.value_slot(|| 9_i32);
        let child_result = session.finish_group_body();
        assert!(child_result.detached_children.is_empty());
        session.end_group();

        let root_result = session.finish_group_body();
        assert!(root_result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let snapshot = harness.table.debug_snapshot();
    assert_eq!(snapshot.active_groups.len(), 2);
    assert_eq!(snapshot.anchors.len(), 2);
    assert_eq!(snapshot.scopes.len(), 2);
    assert_eq!(snapshot.active_payload_count, 2);
    assert_eq!(snapshot.active_node_count, 1);
    assert_eq!(snapshot.active_scope_count, 2);
    assert_eq!(snapshot.scope_registry_count, 2);
    assert_eq!(snapshot.retained_subtree_count, 0);

    let root = snapshot
        .active_groups
        .iter()
        .find(|group| group.static_key == ROOT_KEY)
        .expect("root group snapshot");
    assert_eq!(root.scope_id, Some(ROOT_SCOPE));
    assert_eq!(root.subtree_len, 2);
    assert_eq!(root.payload_len, 1);
    assert_eq!(root.node_len, 1);

    let child = snapshot
        .active_groups
        .iter()
        .find(|group| group.static_key == CHILD_KEY)
        .expect("child group snapshot");
    assert_eq!(child.parent_anchor, root.anchor);
    assert_eq!(child.scope_id, Some(CHILD_SCOPE));
    assert_eq!(child.subtree_len, 1);
    assert_eq!(child.payload_len, 1);
}

#[test]
fn debug_dump_slot_entries_use_v2_native_labels() {
    const GROUP_KEY: Key = 51;

    let table = composed_group_with_value_and_node_table(GROUP_KEY);
    let rows = table.debug_dump_slot_entries();

    assert!(
        rows.iter()
            .any(|entry| entry.kind == super::SlotDebugEntryKind::Payload),
        "payload entries should still be visible in debug output"
    );
    assert!(
        rows.iter()
            .any(|entry| entry.line.contains("kind=internal")),
        "debug output must expose the stored payload classification"
    );
    assert!(
        rows.iter().all(|entry| {
            !entry.line.starts_with("Value(") && !entry.line.starts_with("PayloadKind(")
        }),
        "debug output must speak in V2 entry labels rather than fake slot-stream wrappers"
    );
}

#[test]
fn take_all_drops_reserves_only_payload_count() {
    let mut table = composed_group_with_value_and_node_table(23);
    let drops = table.take_all_drops();

    assert_eq!(drops.len(), 1);
    assert_eq!(drops.capacity(), drops.len());
    assert!(table.groups.is_empty());
    assert!(table.payloads.is_empty());
    assert!(table.nodes.is_empty());
}

#[test]
fn debug_stats_report_explicit_v2_table_counts() {
    const GROUP_KEY: Key = 34;

    let table = composed_group_with_value_and_node_table(GROUP_KEY);
    let stats = table.debug_stats();

    assert_eq!(stats.group_count, 1);
    assert_eq!(stats.payload_count, 1);
    assert_eq!(stats.payload_location_count, 1);
    assert_eq!(stats.node_count, 1);
    assert_eq!(stats.active_anchor_count, 1);
    assert_eq!(stats.group_record_size, mem::size_of::<GroupRecord>());
    assert_eq!(
        stats.group_heap_bytes,
        stats.group_capacity * stats.group_record_size
    );
    assert_eq!(stats.anchor_slot_count, 1);
    assert_eq!(stats.anchor_sparse_count, 0);
    assert_eq!(stats.detached_anchor_count, 0);
    assert_eq!(stats.invalidated_anchor_count, 0);
    assert_eq!(stats.free_anchor_count, 0);
    assert_eq!(stats.scope_index_count, 0);
    assert_eq!(stats.pending_drop_count, 0);
    assert!(stats.group_capacity >= stats.group_count);
    assert!(stats.payload_capacity >= stats.payload_count);
    assert!(stats.payload_location_capacity >= stats.payload_location_count);
    assert!(stats.node_capacity >= stats.node_count);
    assert!(stats.anchor_capacity >= stats.active_anchor_count);
    assert!(stats.anchor_heap_bytes > 0);
    assert!(stats.scope_index_capacity >= stats.scope_index_count);
    assert!(stats.pending_drop_capacity >= stats.pending_drop_count);
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_group_count, 0);
    assert_eq!(stats.retained_payload_count, 0);
    assert_eq!(stats.retained_node_count, 0);
    assert_eq!(stats.retained_scope_count, 0);
    assert_eq!(stats.retained_anchor_count, 0);
    assert_eq!(stats.retained_heap_bytes, 0);
    assert_eq!(stats.retained_evictions_total, 0);
}

#[test]
fn group_record_footprint_stays_bounded_until_profiling_justifies_split_storage() {
    let group_record_size = mem::size_of::<GroupRecord>();

    assert!(
        group_record_size <= 128,
        "GroupRecord is {group_record_size} bytes; revisit field packing with perf data before accepting more group-table bandwidth"
    );
}
