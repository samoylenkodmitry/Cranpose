use super::*;

fn compose_single_i32_value_slot(harness: &mut SlotHarness, key: Key, value: i32) -> ValueSlotId {
    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        begin_unkeyed(session, key, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || value);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();
    slot
}

#[test]
fn slot_storage_ids_do_not_use_process_global_counter() {
    let source = include_str!("../table.rs");
    assert!(!source.contains("NEXT_SLOT_STORAGE_ID"));
    assert!(!source.contains("fetch_add(1, Ordering::Relaxed)"));
}

#[test]
fn payload_records_store_semantic_payload_kinds() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, 52, None);
        let _remembered = session.remember(|| 17_i32);
        let _param_slot = session
            .value_slot_with_kind(super::PayloadKind::Param, crate::ParamState::<i32>::default);
        let _callback_slot =
            session.value_slot_with_kind(super::PayloadKind::Param, crate::CallbackHolder::new);
        let _return_slot = session.value_slot_with_kind(
            super::PayloadKind::Return,
            crate::ReturnSlot::<i32>::default,
        );
        let _effect_slot = session.remember_with_kind(
            super::PayloadKind::Effect,
            crate::DisposableEffectState::default,
        );
        let _type_named_internal_slot =
            session.value_slot_with_kind(PayloadKind::Internal, crate::ReturnSlot::<i32>::default);
        let _internal_slot = session.value_slot_with_kind(PayloadKind::Internal, || 99_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let payload_kinds = harness
        .table
        .group_payload_records_at(0)
        .iter()
        .map(|payload| payload.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        payload_kinds,
        vec![
            super::PayloadKind::Remember,
            super::PayloadKind::Param,
            super::PayloadKind::Param,
            super::PayloadKind::Return,
            super::PayloadKind::Effect,
            super::PayloadKind::Internal,
            super::PayloadKind::Internal,
        ]
    );
}

#[test]
fn payload_cleanup_with_stale_owner_anchor_does_not_panic() {
    const GROUP_KEY: Key = 52_001;

    let mut harness = SlotHarness::new();
    let _slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);
    let owner = harness.table.payload_owner_at(0, 0);

    harness.table.anchors.mark_detached(owner);

    assert!(harness
        .table
        .remove_payload_tail_at_cursor(owner, 0)
        .is_empty());
    harness
        .table
        .refresh_group_payload_anchor_locations(owner, 0);

    assert_eq!(harness.table.total_payload_count(), 1);
}

#[test]
fn payload_segment_insert_rejects_out_of_range_offset_without_mutating() {
    let owner = AnchorId::new(1);
    let mut groups = vec![GroupRecord {
        key: GroupKey::new(52_002, None, 0),
        parent_anchor: AnchorId::INVALID,
        depth: 0,
        subtree_len: 1,
        payload_start: 0,
        payload_len: 1,
        node_start: 0,
        node_len: 0,
        subtree_node_count: 0,
        generation: 0,
        anchor: owner,
        scope_id: None,
        transparent: false,
    }];
    let mut payloads = vec![17_i32];

    crate::slot::segments::insert_group_segment_item::<crate::slot::segments::PayloadSegment, _>(
        &mut groups,
        &mut payloads,
        0,
        usize::MAX,
        23_i32,
    );

    assert_eq!(payloads, vec![17_i32]);
    assert_eq!(groups[0].payload_len, 1);
}

#[test]
fn value_payload_insertion_rejects_exhausted_anchor_ids_without_mutating() {
    const GROUP_KEY: Key = 52_003;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let owner = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        session
            .state
            .group_stack
            .last()
            .expect("test group frame should be active")
            .group_anchor
    });
    harness
        .table
        .set_next_payload_anchor_for_test(u32::MAX as usize + 1);

    let payload = harness
        .table
        .insert_value_payload(owner, 0, PayloadKind::Internal, 17_i32);

    assert_eq!(payload, PayloadAnchor::INVALID);
    assert!(harness.table.group_payload_records_at(0).is_empty());
    assert_eq!(harness.table.total_payload_count(), 0);
    let result = harness.session(|session| {
        let result = session.finish_group_body();
        session.end_group();
        result
    });
    assert!(result.detached_children.is_empty());
    harness.finish_pass();
}

#[test]
fn payload_range_with_mismatched_owner_is_ignored() {
    const PARENT_KEY: Key = 52_011;
    const FIRST_CHILD_KEY: Key = 52_012;
    const SECOND_CHILD_KEY: Key = 52_013;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, second_slot) = harness.session(|session| {
        begin_unkeyed(session, PARENT_KEY, None);

        begin_unkeyed(session, FIRST_CHILD_KEY, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 17_i32);
        let first_result = session.finish_group_body();
        assert!(first_result.detached_children.is_empty());
        session.end_group();

        begin_unkeyed(session, SECOND_CHILD_KEY, None);
        let second_slot = session.value_slot_with_kind(PayloadKind::Internal, || 29_i32);
        let second_result = session.finish_group_body();
        assert!(second_result.detached_children.is_empty());
        session.end_group();

        let parent_result = session.finish_group_body();
        assert!(parent_result.detached_children.is_empty());
        session.end_group();

        (first_slot, second_slot)
    });
    harness.finish_pass();

    let first_owner = harness.table.groups[1].anchor;
    let second_range = harness.table.group_payload_subrange_at(2, 0, 1);

    let removed = harness
        .table
        .remove_payload_range(first_owner, second_range);

    assert!(removed.is_empty());
    assert_eq!(harness.table.total_payload_count(), 2);
    assert_eq!(*harness.table.read_value::<i32>(first_slot), 17);
    assert_eq!(*harness.table.read_value::<i32>(second_slot), 29);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn payload_range_outside_current_group_segment_is_ignored() {
    const GROUP_KEY: Key = 52_014;

    let mut harness = SlotHarness::new();
    let slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);
    let owner = harness.table.groups[0].anchor;
    let stale_range = crate::slot::GroupPayloadRange::from_range(
        crate::slot::ranges::GroupItemRange::new(0, crate::slot::PayloadRange::new(0, 2), 0, 2),
        0,
    );

    let removed = harness.table.remove_payload_range(owner, stale_range);

    assert!(removed.is_empty());
    assert_eq!(harness.table.total_payload_count(), 1);
    assert_eq!(*harness.table.read_value::<i32>(slot), 17);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn payload_tail_cleanup_repairs_corrupt_group_payload_len() {
    const GROUP_KEY: Key = 52_002;

    let mut harness = SlotHarness::new();
    let original = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);
    harness.table.groups[0].payload_len = 2;

    harness.begin_pass(SlotPassMode::Compose);
    let reused = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 99_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(reused, original);
    assert_eq!(harness.table.groups[0].payload_len, 1);
    assert_eq!(harness.table.total_payload_count(), 1);
    assert_eq!(*harness.table.read_value::<i32>(reused), 17);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn payload_subrange_with_corrupt_segment_start_is_empty() {
    const GROUP_KEY: Key = 52_003;

    let mut harness = SlotHarness::new();
    let _slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);

    harness.table.groups[0].payload_start = u32::MAX;

    let range = harness.table.group_payload_subrange_at(0, 0, 1);

    assert!(range.is_empty());
    assert_eq!(harness.table.total_payload_count(), 1);
}

#[test]
fn payload_location_refresh_ignores_corrupt_group_payload_range() {
    const GROUP_KEY: Key = 52_004;

    let mut harness = SlotHarness::new();
    let _slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);
    let owner = harness.table.payload_owner_at(0, 0);

    harness.table.groups[0].payload_start = u32::MAX;

    harness
        .table
        .refresh_group_payload_anchor_locations(owner, 0);
    assert!(harness.table.group_payload_records_at(0).is_empty());
    assert_eq!(harness.table.total_payload_count(), 1);
}

#[test]
fn value_payload_reuses_after_corrupt_group_payload_start_repair() {
    const GROUP_KEY: Key = 52_005;

    let mut harness = SlotHarness::new();
    let _slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 17);

    harness.table.groups[0].payload_start = u32::MAX;

    harness.begin_pass(SlotPassMode::Compose);
    let repaired = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 99_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(*harness.table.read_value::<i32>(repaired), 17);
    assert_eq!(harness.table.groups[0].payload_len, 1);
    assert_eq!(harness.table.groups[0].payload_start, 0);
    assert_eq!(harness.table.total_payload_count(), 1);
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn appended_value_slots_batch_payload_location_refresh() {
    const GROUP_KEY: Key = 53;

    let mut harness = SlotHarness::new();
    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    let slots = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let first = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let remembered = session.remember(|| 20_i32);
        let third = session.value_slot_with_kind(PayloadKind::Internal, || 30_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (first, remembered, third)
    });
    harness.finish_pass();

    let after = harness.table.debug_stats();
    assert_eq!(after.active_payload_anchor_count, 3);
    assert_eq!(
        after.mutation.payload_location_refresh_count - before.payload_location_refresh_count,
        1
    );
    assert_eq!(
        after.mutation.payload_location_refresh_payload_count
            - before.payload_location_refresh_payload_count,
        3
    );
    assert_eq!(after.mutation.payload_location_refresh_max_span, 3);
    assert_eq!(*harness.table.read_value::<i32>(slots.0), 10);
    assert_eq!(slots.1.with(|value| *value), 20);
    assert_eq!(*harness.table.read_value::<i32>(slots.2), 30);
}

#[test]
fn pending_payload_location_refreshes_coalesce_to_lowest_start() {
    const GROUP_KEY: Key = 56;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let _first = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let (first, second, third) = harness.session(|session| {
        let group = begin_unkeyed(session, GROUP_KEY, None);
        let first = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        assert!(
            !session.state.has_pending_payload_location_refreshes(),
            "reusing an existing payload must not request a location refresh"
        );

        let second = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        assert_eq!(
            session
                .state
                .pending_payload_location_refresh_start(group.anchor),
            Some(1)
        );

        let third = session.value_slot_with_kind(PayloadKind::Internal, || 30_i32);
        assert_eq!(
            session
                .state
                .pending_payload_location_refresh_start(group.anchor),
            Some(1),
            "multiple payload inserts in one group must coalesce to the lowest dirty index"
        );

        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert!(
            !session.state.has_pending_payload_location_refreshes(),
            "finish_group_body must flush deferred payload location refreshes"
        );
        assert_eq!(*session.table.read_value::<i32>(first), 10);
        assert_eq!(*session.table.read_value::<i32>(second), 20);
        assert_eq!(*session.table.read_value::<i32>(third), 30);
        session.end_group();
        (first, second, third)
    });
    harness.finish_pass();

    assert_eq!(*harness.table.read_value::<i32>(first), 10);
    assert_eq!(*harness.table.read_value::<i32>(second), 20);
    assert_eq!(*harness.table.read_value::<i32>(third), 30);
}

#[test]
fn writer_validation_rejects_pending_payload_location_refreshes() {
    const GROUP_KEY: Key = 57;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let slot = {
        let mut session = harness
            .table
            .write_session(&mut harness.lifecycle, &mut harness.state);
        begin_unkeyed(&mut session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 42_i32);
        assert!(
            session.state.has_pending_payload_location_refreshes(),
            "appending a payload should defer the range refresh until a writer boundary"
        );
        slot
    };
    assert_eq!(
        harness.state.validate(&harness.table),
        Err(SlotInvariantError::WriterPendingPayloadLocationRefreshes { count: 1 })
    );

    harness
        .state
        .flush_payload_location_refreshes(&mut harness.table);
    assert!(!harness.state.has_pending_payload_location_refreshes());
    assert_eq!(harness.state.validate(&harness.table), Ok(()));
    harness.finish_pass();

    assert_eq!(*harness.table.read_value::<i32>(slot), 42);
}

#[test]
fn payload_tail_removal_does_not_refresh_empty_suffix() {
    const GROUP_KEY: Key = 55;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let _first = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let _second = session.value_slot_with_kind(PayloadKind::Internal, || 20_i32);
        let _third = session.value_slot_with_kind(PayloadKind::Internal, || 30_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let before = harness.table.debug_stats().mutation;

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let _first = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    let after = harness.table.debug_stats().mutation;
    assert_eq!(
        after.payload_location_refresh_count - before.payload_location_refresh_count,
        0
    );
    assert_eq!(harness.table.debug_stats().active_payload_anchor_count, 1);
}

#[test]
fn payload_kind_updates_when_same_type_slot_changes_semantics() {
    const GROUP_KEY: Key = 54;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let first_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(super::PayloadKind::Param, || 7_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();
    assert_eq!(
        harness.table.group_payload_record_at(0, 0).kind,
        super::PayloadKind::Param
    );

    harness.begin_pass(SlotPassMode::Compose);
    let second_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(super::PayloadKind::Return, || 9_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(first_slot, second_slot);
    assert_eq!(
        harness.table.group_payload_record_at(0, 0).kind,
        super::PayloadKind::Return
    );
    assert_eq!(*harness.table.read_value::<i32>(second_slot), 7);
}

#[test]
fn second_identical_composition_reuses_group_and_value() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let first_slot = harness.session(|session| {
        begin_unkeyed(session, 11, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 10_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();
    harness.table.write_value(first_slot, 99_i32);

    harness.begin_pass(SlotPassMode::Compose);
    let (kind, second_slot) = harness.session(|session| {
        let started = begin_unkeyed(session, 11, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (started.kind, slot)
    });
    harness.finish_pass();

    assert_eq!(kind, GroupStartKind::Reused);
    assert_eq!(second_slot, first_slot);
    assert_eq!(*harness.table.read_value::<i32>(second_slot), 99);
}

#[test]
fn read_value_mut_updates_existing_slot_in_place() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        begin_unkeyed(session, 12, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    *harness.table.read_value_mut::<i32>(slot) = 77;

    assert_eq!(*harness.table.read_value::<i32>(slot), 77);
}

#[test]
fn same_table_value_slot_read_write_respects_storage_identity() {
    let mut harness = SlotHarness::new();

    let slot = compose_single_i32_value_slot(&mut harness, 17, 21);

    assert_eq!(slot.storage_id(), harness.table.storage_id());
    assert_eq!(*harness.table.read_value::<i32>(slot), 21);
    harness.table.write_value(slot, 34_i32);
    assert_eq!(*harness.table.read_value::<i32>(slot), 34);
}

#[test]
fn cross_table_value_slot_read_reports_foreign_table_before_anchor_resolution() {
    let mut first = SlotHarness::new();
    let first_slot = compose_single_i32_value_slot(&mut first, 18, 55);

    let mut second = SlotHarness::new();
    let second_slot = compose_single_i32_value_slot(&mut second, 20, 89);

    assert_eq!(first_slot.anchor(), second_slot.anchor());
    assert_ne!(first_slot.storage_id(), second_slot.storage_id());
    assert_eq!(
        second.table.try_read_value::<i32>(first_slot),
        Err(ValueSlotError::ForeignTable {
            table_storage_id: second.table.storage_id(),
            slot_storage_id: first_slot.storage_id(),
        })
    );
    assert_eq!(*first.table.read_value::<i32>(first_slot), 55);
    assert_eq!(*second.table.read_value::<i32>(second_slot), 89);
}

#[test]
fn cross_table_value_slot_write_reports_foreign_table_before_anchor_resolution() {
    let mut first = SlotHarness::new();
    let first_slot = compose_single_i32_value_slot(&mut first, 21, 55);

    let mut second = SlotHarness::new();
    let second_slot = compose_single_i32_value_slot(&mut second, 22, 89);

    assert_eq!(first_slot.anchor(), second_slot.anchor());
    assert_ne!(first_slot.storage_id(), second_slot.storage_id());
    assert_eq!(
        second.table.try_write_value(first_slot, 144_i32),
        Err(ValueSlotError::ForeignTable {
            table_storage_id: second.table.storage_id(),
            slot_storage_id: first_slot.storage_id(),
        })
    );
    assert_eq!(*first.table.read_value::<i32>(first_slot), 55);
    assert_eq!(*second.table.read_value::<i32>(second_slot), 89);
}

#[test]
fn read_value_type_mismatch_panics_consistently() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        begin_unkeyed(session, 15, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    let mismatch_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<u32>(slot);
    }));
    assert!(
        mismatch_read.is_err(),
        "typed value-slot reads must fail on a type mismatch"
    );
}

#[test]
fn value_slot_type_replacement_advances_generation() {
    const GROUP_KEY: Key = 16;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let old_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let replacement_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 7_u32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(replacement_slot.anchor().id(), old_slot.anchor().id());
    assert_ne!(
        replacement_slot.anchor().generation(),
        old_slot.anchor().generation()
    );
    assert_eq!(*harness.table.read_value::<u32>(replacement_slot), 7);

    let stale_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(old_slot);
    }));
    assert!(
        stale_read.is_err(),
        "replaced value slot handles must fail instead of reading the replacement"
    );
}

#[test]
fn value_slot_type_replacement_recovers_stale_payload_anchor_record() {
    const GROUP_KEY: Key = 58;

    let mut harness = SlotHarness::new();
    let first_slot = compose_single_i32_value_slot(&mut harness, GROUP_KEY, 7);
    let stale_anchor = harness.table.payload_anchor_at(0, 0);
    assert_eq!(first_slot.anchor(), stale_anchor);
    assert!(harness.table.payload_anchors.invalidate(stale_anchor));

    harness.begin_pass(SlotPassMode::Compose);
    let second_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 11_u32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_ne!(
        second_slot.anchor(),
        stale_anchor,
        "stale payload anchor records must be replaced with a fresh active anchor"
    );
    assert_eq!(
        harness.table.payload_anchor_active_location(stale_anchor),
        None
    );
    assert_eq!(
        harness
            .table
            .payload_anchor_active_location(second_slot.anchor()),
        Some((harness.table.payload_owner_at(0, 0), 0))
    );
    assert_eq!(*harness.table.read_value::<u32>(second_slot), 11);
}

#[test]
fn disposed_value_slot_handle_does_not_alias_new_slot() {
    const OLD_KEY: Key = 13;
    const NEW_KEY: Key = 14;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let old_slot = harness.session(|session| {
        begin_unkeyed(session, OLD_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();
    assert!(harness.table.groups.is_empty());
    harness.table.compact_payload_anchor_registry_storage(None);

    harness.begin_pass(SlotPassMode::Compose);
    let new_slot = harness.session(|session| {
        begin_unkeyed(session, NEW_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 77_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(new_slot.anchor().id(), old_slot.anchor().id());
    assert_ne!(
        new_slot.anchor().generation(),
        old_slot.anchor().generation()
    );
    assert_eq!(*harness.table.read_value::<i32>(new_slot), 77);

    let stale_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(old_slot);
    }));
    assert!(
        stale_read.is_err(),
        "disposed value slot handles must fail cleanly instead of aliasing a new slot"
    );
}

#[test]
fn removed_payload_tail_invalidates_value_slot_handle() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, removed_slot) = harness.session(|session| {
        begin_unkeyed(session, 19, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 1_i32);
        let removed_slot = session.value_slot_with_kind(PayloadKind::Internal, || 2_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        (first_slot, removed_slot)
    });
    harness.finish_pass();

    let owner = harness.table.groups[0].anchor;
    let removed_range = harness.table.group_payload_subrange_at(0, 1, 2);
    let removed = harness.table.remove_payload_range(owner, removed_range);
    assert_eq!(removed.len(), 1);

    assert_eq!(*harness.table.read_value::<i32>(first_slot), 1);
    let removed_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(removed_slot);
    }));
    assert!(
        removed_read.is_err(),
        "removed payload tail handles must fail cleanly after invalidation"
    );
    assert_eq!(harness.table.validate(), Ok(()));
}

#[test]
fn recomposition_removes_tail_payloads_and_nodes_together() {
    const GROUP_KEY: Key = 59;
    const FIRST_NODE: NodeId = 590;
    const SECOND_NODE: NodeId = 591;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let (first_slot, removed_slot) = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let first_slot = session.value_slot_with_kind(PayloadKind::Internal, || 1_i32);
        let removed_slot = session.value_slot_with_kind(PayloadKind::Internal, || 2_i32);
        session.record_node_with_parent(FIRST_NODE, 1, None);
        session.record_node_with_parent(SECOND_NODE, 1, None);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        assert!(result.direct_nodes.is_empty());
        session.end_group();
        (first_slot, removed_slot)
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let direct_nodes = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let first_slot_after = session.value_slot_with_kind(PayloadKind::Internal, || 0_i32);
        assert_eq!(first_slot_after, first_slot);
        let recorded = session.record_node_with_parent(FIRST_NODE, 1, None);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Reused {
                id: FIRST_NODE,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        result.direct_nodes
    });
    harness.finish_pass();

    assert_eq!(direct_nodes, vec![SECOND_NODE]);
    assert_eq!(harness.table.group_payload_len_at(0), 1);
    assert_eq!(harness.table.group_node_len_at(0), 1);
    assert_eq!(*harness.table.read_value::<i32>(first_slot), 1);
    let removed_read = panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(removed_slot);
    }));
    assert!(
        removed_read.is_err(),
        "tail payload removal must invalidate the removed value slot"
    );
    assert_eq!(harness.table.group_node_record_at(0, 0).id, FIRST_NODE);
    assert_eq!(harness.table.validate(), Ok(()));
}
