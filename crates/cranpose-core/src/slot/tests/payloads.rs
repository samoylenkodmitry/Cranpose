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
fn writer_validation_flushes_pending_payload_location_refreshes() {
    const GROUP_KEY: Key = 57;

    let mut harness = SlotHarness::new();
    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot_with_kind(PayloadKind::Internal, || 42_i32);
        assert!(
            session.state.has_pending_payload_location_refreshes(),
            "appending a payload should defer the range refresh until a writer boundary"
        );
        slot
    });
    assert!(
        !harness.state.has_pending_payload_location_refreshes(),
        "writer validation must flush deferred payload location refreshes"
    );
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

    assert_eq!(*harness.table.read_value::<i32>(slot), 21);
    harness.table.write_value(slot, 34_i32);
    assert_eq!(*harness.table.read_value::<i32>(slot), 34);
}

#[test]
#[should_panic(expected = "value slot belongs to a different slot table")]
fn cross_table_value_slot_read_panics_before_anchor_resolution() {
    let mut first = SlotHarness::new();
    let first_slot = compose_single_i32_value_slot(&mut first, 18, 55);

    let mut second = SlotHarness::new();
    let second_slot = compose_single_i32_value_slot(&mut second, 20, 89);

    assert_eq!(first_slot.anchor(), second_slot.anchor());
    let _ = second.table.read_value::<i32>(first_slot);
}

#[test]
fn cross_table_value_slot_write_does_not_alias_matching_anchor() {
    let mut first = SlotHarness::new();
    let first_slot = compose_single_i32_value_slot(&mut first, 21, 55);

    let mut second = SlotHarness::new();
    let second_slot = compose_single_i32_value_slot(&mut second, 22, 89);

    assert_eq!(first_slot.anchor(), second_slot.anchor());
    let cross_table_write = panic::catch_unwind(AssertUnwindSafe(|| {
        second.table.write_value(first_slot, 144_i32);
    }));
    assert!(
        cross_table_write.is_err(),
        "cross-table value-slot writes must fail before touching matching anchors"
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
