use super::*;

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
        let _type_named_internal_slot = session.value_slot(crate::ReturnSlot::<i32>::default);
        let _internal_slot = session.value_slot(|| 99_i32);
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
        let slot = session.value_slot(|| 10_i32);
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
        let slot = session.value_slot(|| 0_i32);
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
        let slot = session.value_slot(|| 5_i32);
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
fn read_value_type_mismatch_panics_consistently() {
    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    let slot = harness.session(|session| {
        begin_unkeyed(session, 15, None);
        let slot = session.value_slot(|| 5_i32);
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
        let slot = session.value_slot(|| 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    let replacement_slot = harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let slot = session.value_slot(|| 7_u32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(replacement_slot.anchor(), old_slot.anchor());
    assert_ne!(replacement_slot.generation(), old_slot.generation());
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
        let slot = session.value_slot(|| 5_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.finish_pass();
    assert!(harness.table.groups.is_empty());
    harness.table.compact_payload_anchor_namespace(None);

    harness.begin_pass(SlotPassMode::Compose);
    let new_slot = harness.session(|session| {
        begin_unkeyed(session, NEW_KEY, None);
        let slot = session.value_slot(|| 77_i32);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
        slot
    });
    harness.finish_pass();

    assert_eq!(new_slot.anchor(), old_slot.anchor());
    assert_ne!(new_slot.generation(), old_slot.generation());
    assert_eq!(*harness.table.read_value::<i32>(new_slot), 77);

    let stale_read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = harness.table.read_value::<i32>(old_slot);
    }));
    assert!(
        stale_read.is_err(),
        "disposed value slot handles must fail cleanly instead of aliasing a new slot"
    );
}
