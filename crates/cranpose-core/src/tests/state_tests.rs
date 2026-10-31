use super::*;

fn writable_record(
    state: &SnapshotMutableState<i32>,
    snapshot_id: SnapshotId,
    invalid: &SnapshotIdSet,
) -> Rc<StateRecord> {
    WriteTarget {
        records: &state.records,
        id: state.id,
    }
    .writable_record(snapshot_id, invalid)
}

fn create_record_chain(ids: &[SnapshotId]) -> RecordChain {
    RecordChain::new(ids.iter().map(|&id| StateRecord::new(id, 0i32)).collect())
}

struct ManualState {
    records: RecordChain,
}

impl ManualState {
    fn new(records: SmallVec<[Rc<StateRecord>; 2]>) -> Self {
        Self {
            records: RecordChain::new(records),
        }
    }
}

impl StateObject for ManualState {
    fn object_id(&self) -> ObjectId {
        ObjectId(999)
    }

    fn records(&self) -> &RecordChain {
        &self.records
    }

    fn try_readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Option<Rc<StateRecord>> {
        Some(self.records.head())
    }

    fn readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Rc<StateRecord> {
        self.records.head()
    }

    fn promote_record(&self, _: SnapshotId) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[test]
fn observation_leases_drive_subscriber_liveness() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let notifications = Rc::new(Cell::new(0));
    let notifications_for_callback = Rc::clone(&notifications);
    let callback: Rc<dyn Fn()> = Rc::new(move || {
        notifications_for_callback.set(notifications_for_callback.get() + 1);
    });
    state
        .subscribers
        .subscriber_callback(Rc::clone(&callback), false);

    let first = StateObject::observation_lease(&*state).expect("first observation lease");
    let second = StateObject::observation_lease(&*state).expect("second observation lease");
    assert!(state.subscribers.has_subscribers());
    assert_eq!(notifications.get(), 1);

    drop(first);
    assert!(state.subscribers.has_subscribers());
    drop(second);
    assert!(!state.subscribers.has_subscribers());

    let third = StateObject::observation_lease(&*state).expect("third observation lease");
    assert_eq!(notifications.get(), 2);
    drop(third);
}

#[test]
fn observation_leases_preserve_clones_scope_counts_and_state_lifetimes() {
    let first = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let second = SnapshotMutableState::new_in_rc(200i32, Rc::new(NeverEqual));
    let lease = first.observation_lease().unwrap();
    let clone = Rc::clone(&lease);
    assert!(first.subscribers.has_subscribers());
    assert!(!second.subscribers.has_subscribers());

    drop(lease);
    assert!(first.subscribers.has_subscribers());
    assert!(!first.subscribers.add_scope_observer());
    drop(clone);
    assert!(first.subscribers.has_subscribers());
    first.subscribers.remove_scope_observers(1);
    assert!(!first.subscribers.has_subscribers());

    let lease = second.observation_lease().unwrap();
    let weak = Rc::downgrade(&second);
    drop(second);
    assert!(weak.upgrade().is_none());
    drop(lease);
    assert!(!first.subscribers.has_subscribers());
}

#[test]
fn snapshot_mutable_state_promote_missing_record_returns_error() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let missing_snapshot = SnapshotId(usize::MAX - 17);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        StateObject::promote_record(&*state, missing_snapshot)
    }));

    assert!(
        matches!(result, Ok(Err("missing child record"))),
        "missing child record should be reported through Result, got {result:?}"
    );
}

#[test]
fn snapshot_mutable_state_promote_wrong_record_type_returns_error() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let child_snapshot = SnapshotId(usize::MAX - 31);
    let wrong_record = StateRecord::new(child_snapshot, "wrong type");
    state.records().prepend(wrong_record);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        StateObject::promote_record(&*state, child_snapshot)
    }));

    assert!(
        matches!(result, Ok(Err("child record value missing or wrong type"))),
        "wrong child record type should be reported through Result, got {result:?}"
    );
}

#[test]
fn snapshot_mutable_state_commit_wrong_record_type_returns_error() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let merged = StateRecord::new(SnapshotId(usize::MAX - 43), "wrong type");

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        StateObject::commit_merged_record(&*state, merged)
    }));

    assert!(
        matches!(result, Ok(Err("merged record value missing or wrong type"))),
        "wrong merged record type should be reported through Result, got {result:?}"
    );
}

#[test]
fn snapshot_mutable_state_merge_wrong_record_type_returns_none() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let previous = StateRecord::new(SnapshotId(usize::MAX - 51), 1i32);
    let current = StateRecord::new(SnapshotId(usize::MAX - 52), "wrong type");
    let applied = StateRecord::new(SnapshotId(usize::MAX - 53), 2i32);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        StateObject::merge_records(&*state, previous, current, applied)
    }));

    match result {
        Ok(None) => {}
        Ok(Some(_)) => panic!("wrong merge record type unexpectedly produced a merged record"),
        Err(_) => panic!("wrong merge record type should not panic"),
    }
}

#[test]
fn test_used_locked_finds_invalid_snapshot() {
    let records = RecordChain::new(smallvec![
        StateRecord::new(SnapshotId(10), 0i32),
        StateRecord::new(INVALID_SNAPSHOT_ID, 0i32),
        StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32),
    ]);

    let result = records.used();
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_used_locked_finds_obscured_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let pin_handle = crate::snapshot_pinning::track_pinning(SnapshotId(10), &SnapshotIdSet::EMPTY);

    let records = RecordChain::new(smallvec![
        StateRecord::new(SnapshotId(100), 0i32),
        StateRecord::new(SnapshotId(5), 0i32),
        StateRecord::new(SnapshotId(2), 0i32),
    ]);

    let result = records.used();

    assert!(result.is_some());
    let reused = result.unwrap();
    assert_eq!(
        reused.snapshot_id(),
        SnapshotId(2),
        "Should return the oldest obscured record"
    );

    crate::snapshot_pinning::release_pinning(pin_handle);
}

#[test]
fn test_used_locked_no_reusable_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let high_id = SnapshotId::new(allocate_record_id().index() + 1000);
    let records = create_record_chain(&[high_id, high_id.next(), high_id.next().next()]);

    let result = records.used();
    assert!(
        result.is_none(),
        "Should find no reusable records when all are recent"
    );
}

#[test]
fn test_used_locked_single_old_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let records = RecordChain::new(smallvec![
        StateRecord::new(SnapshotId(100), 0i32),
        StateRecord::new(SnapshotId(2), 0i32),
    ]);

    let result = records.used();
    assert!(result.is_none(), "Single old record should not be reused");
}

#[test]
fn test_readable_record_for_preexisting() {
    let head = create_record_chain(&[PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::EMPTY;

    let result = head.readable(SnapshotId(10), &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
}

#[test]
fn test_readable_record_for_picks_highest_valid() {
    let head = create_record_chain(&[SnapshotId(10), SnapshotId(5), PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::EMPTY;

    let result = head.readable(SnapshotId(10), &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), SnapshotId(10));

    let result = head.readable(SnapshotId(7), &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), SnapshotId(5));
}

#[test]
fn test_new_overwritable_record_locked_reuses_invalid() {
    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));

    let invalid_rec = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32);
    state.records().prepend(Rc::clone(&invalid_rec));

    let result = state.records().overwritable();

    assert!(Rc::ptr_eq(&result, &invalid_rec));
    assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);
}

#[test]
fn test_new_overwritable_record_locked_creates_new() {
    crate::snapshot_pinning::reset_pinning_table();

    let _pin_handle = crate::snapshot_pinning::track_pinning(SnapshotId(1), &SnapshotIdSet::EMPTY);

    let state = SnapshotMutableState::new_in_rc(100i32, Rc::new(NeverEqual));
    let old_head = state.records().head();

    let result = state.records().overwritable();

    assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);

    let new_head = state.records().head();
    assert!(
        Rc::ptr_eq(&new_head, &result),
        "new_head ({:p}) should equal result ({:p})",
        Rc::as_ptr(&new_head),
        Rc::as_ptr(&result)
    );

    assert_eq!(
        state.records().ids()[1],
        (old_head.snapshot_id(), false),
        "the old head follows the new one"
    );
}

#[test]
fn test_writable_record_reuses_invalid_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_rc(7i32, Rc::new(NeverEqual));

    let invalid = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32);
    state.records().prepend(Rc::clone(&invalid));

    let snapshot_id = allocate_record_id();
    let result = writable_record(&state, snapshot_id, &SnapshotIdSet::EMPTY);

    assert!(
        Rc::ptr_eq(&result, &invalid),
        "Expected writable_record to reuse the INVALID record"
    );
    assert_eq!(result.snapshot_id(), snapshot_id);
    result.with_value(|value: &i32| {
        assert_eq!(*value, 7, "Reused record should copy the readable value");
    });
    assert!(!result.is_tombstone());
}

#[test]
fn test_writable_record_creates_new_when_reuse_disallowed() {
    crate::snapshot_pinning::reset_pinning_table();
    let pin = crate::snapshot_pinning::track_pinning(SnapshotId(1), &SnapshotIdSet::EMPTY);

    let state = SnapshotMutableState::new_in_rc(42i32, Rc::new(NeverEqual));
    let original_head = state.records().head();
    let preexisting = state
        .records()
        .at(PREEXISTING_SNAPSHOT_ID)
        .expect("preexisting record should exist for newly created state");

    let snapshot_id = allocate_record_id();
    let result = writable_record(&state, snapshot_id, &SnapshotIdSet::EMPTY);

    assert!(
        !Rc::ptr_eq(&result, &original_head),
        "Should not reuse the current head when reuse is disallowed"
    );
    assert!(
        !Rc::ptr_eq(&result, &preexisting),
        "Should not reuse the PREEXISTING record"
    );
    assert_eq!(result.snapshot_id(), snapshot_id);
    result.with_value(|value: &i32| assert_eq!(*value, 42));

    let new_head = state.records().head();
    assert!(
        Rc::ptr_eq(&new_head, &result),
        "Newly created record should become the head of the chain"
    );

    crate::snapshot_pinning::release_pinning(pin);
}

#[test]
fn test_state_record_clear_for_reuse() {
    let record = StateRecord::new(SnapshotId(10), 42i32);

    record.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    record.clear_for_reuse();

    assert_eq!(record.snapshot_id(), SnapshotId(10));
}

#[test]
fn test_overwrite_unused_records_no_old_records() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_rc(42i32, Rc::new(NeverEqual));

    let _pin = crate::snapshot_pinning::track_pinning(SnapshotId(1), &SnapshotIdSet::EMPTY);

    let should_retain = state.overwrite_unused_records();

    assert!(
        should_retain,
        "Should retain multiple records when none are old enough"
    );

    for (id, _) in state.records().ids() {
        assert_ne!(id, INVALID_SNAPSHOT_ID);
    }
}

#[test]
fn test_overwrite_unused_records_basic_cleanup() {
    crate::snapshot_pinning::reset_pinning_table();

    let rec1 = StateRecord::new(SnapshotId(100), 1i32);
    let rec2 = StateRecord::new(SnapshotId(200), 2i32);
    let rec3 = StateRecord::new(SnapshotId(300), 3i32);

    let test_state = ManualState::new(smallvec![
        Rc::clone(&rec3),
        Rc::clone(&rec2),
        Rc::clone(&rec1),
    ]);

    let _pin = crate::snapshot_pinning::track_pinning(SnapshotId(1000), &SnapshotIdSet::EMPTY);

    let result = test_state.records().overwrite_unused();

    assert_eq!(rec3.snapshot_id(), SnapshotId(300));
    assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
    assert_eq!(rec1.snapshot_id(), INVALID_SNAPSHOT_ID);

    assert!(!result);
}

#[test]
fn test_overwrite_unused_records_single_record_only() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = ManualState::new(smallvec![StateRecord::new(SnapshotId(2), 42i32)]);

    let should_retain = state.records().overwrite_unused();

    assert!(!should_retain, "Single record should return false");
}

#[test]
fn snapshot_state_try_get_reports_missing_visible_record_without_panicking() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_rc(42i32, Rc::new(NeverEqual));
    state.records().head().set_snapshot_id(SNAPSHOT_ID_MAX);
    state
        .records()
        .at(PREEXISTING_SNAPSHOT_ID)
        .expect("a new state keeps its preexisting record")
        .set_tombstone(true);

    assert_eq!(state.try_get(), None);
}

#[test]
fn test_overwrite_unused_records_clears_values() {
    crate::snapshot_pinning::reset_pinning_table();

    let old_rec1 = StateRecord::new(SnapshotId(2), 999i32);
    let state = ManualState::new(smallvec![
        StateRecord::new(SnapshotId(150), 42i32),
        StateRecord::new(SnapshotId(3), 888i32),
        Rc::clone(&old_rec1),
        StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32),
    ]);

    old_rec1.with_value(|val: &i32| {
        assert_eq!(*val, 999);
    });

    let _pin = crate::snapshot_pinning::track_pinning(SnapshotId(100), &SnapshotIdSet::EMPTY);
    state.records().overwrite_unused();

    assert_eq!(old_rec1.snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_overwrite_unused_records_mixed_old_and_new() {
    crate::snapshot_pinning::reset_pinning_table();

    let rec2 = StateRecord::new(SnapshotId(2), 100i32);
    let rec5 = StateRecord::new(SnapshotId(5), 100i32);
    let rec50 = StateRecord::new(SnapshotId(50), 100i32);
    let state = ManualState::new(smallvec![
        StateRecord::new(SnapshotId(120), 100i32),
        Rc::clone(&rec50),
        Rc::clone(&rec5),
        Rc::clone(&rec2),
        StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32),
    ]);

    let _pin = crate::snapshot_pinning::track_pinning(SnapshotId(40), &SnapshotIdSet::EMPTY);

    let should_retain = state.records().overwrite_unused();
    assert!(should_retain);

    assert_eq!(rec50.snapshot_id(), SnapshotId(50));
    assert_eq!(rec5.snapshot_id(), SnapshotId(5));
    assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_readable_record_for_skips_invalid_set() {
    let head = create_record_chain(&[SnapshotId(10), SnapshotId(5), PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::new().set(SnapshotId(5));

    let result = head.readable(SnapshotId(10), &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), SnapshotId(10));

    let result = head.readable(SnapshotId(7), &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
}

#[test]
fn test_assign_value_copies_int() {
    let source = StateRecord::new(SnapshotId(10), 42i32);
    let target = StateRecord::new(SnapshotId(20), 0i32);

    target.assign_value(&source).expect("copy int value");

    target.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    source.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    assert_eq!(source.snapshot_id(), SnapshotId(10));
    assert_eq!(target.snapshot_id(), SnapshotId(20));
}

#[test]
fn test_assign_value_copies_string() {
    let source = StateRecord::new(SnapshotId(10), "hello".to_string());
    let target = StateRecord::new(SnapshotId(20), "world".to_string());

    target.assign_value(&source).expect("copy string value");

    target.with_value(|val: &String| {
        assert_eq!(val, "hello");
    });

    source.with_value(|val: &String| {
        assert_eq!(val, "hello");
    });
}

#[test]
fn test_assign_value_reports_cleared_source() {
    let source = StateRecord::new(SnapshotId(10), 42i32);
    let target = StateRecord::new(SnapshotId(20), 0i32);

    source.clear_for_reuse();

    assert_eq!(
        target.assign_value(&source),
        Err(StateRecordValueError::Missing)
    );
    assert_eq!(target.with_value(|val: &i32| *val), 0);
}

#[test]
fn test_assign_value_overwrites_existing_value() {
    let source = StateRecord::new(SnapshotId(10), 100i32);
    let target = StateRecord::new(SnapshotId(20), 999i32);

    target.with_value(|val: &i32| {
        assert_eq!(*val, 999);
    });

    target.assign_value(&source).expect("overwrite int value");

    target.with_value(|val: &i32| {
        assert_eq!(*val, 100);
    });
}

#[test]
fn test_assign_value_with_custom_type() {
    #[derive(Clone, PartialEq, Debug)]
    struct Point {
        x: f64,
        y: f64,
    }

    let source = StateRecord::new(SnapshotId(10), Point { x: 1.5, y: 2.5 });
    let target = StateRecord::new(SnapshotId(20), Point { x: 0.0, y: 0.0 });

    target.assign_value(&source).expect("copy point value");

    target.with_value(|val: &Point| {
        assert_eq!(val, &Point { x: 1.5, y: 2.5 });
    });
}

#[test]
fn test_assign_value_self_assignment() {
    let record = StateRecord::new(SnapshotId(10), 42i32);

    record.assign_value(&record).expect("self-assign int value");

    record.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });
}

#[test]
fn event_loop_writes_keep_the_record_chain_bounded() {
    crate::snapshot_pinning::reset_pinning_table();
    let state = SnapshotMutableState::new_in_rc(0.0f32, Rc::new(NeverEqual));

    let mut lens = Vec::new();
    for event in 0..3000usize {
        crate::run_in_mutable_snapshot(|| {
            state.set(event as f32);
        })
        .expect("event snapshot applies");
        let _ = state.get();
        if event % 500 == 499 {
            lens.push(state.records.ids().len());
        }
    }

    let final_len = *lens.last().expect("sampled chain lengths");
    assert!(
        final_len <= 16,
        "record chain grew without bound across event-loop writes: {lens:?}"
    );
}

#[test]
fn global_writes_keep_the_state_arena_flat() {
    let runtime = runtime::TestRuntime::new();
    let handle = runtime.handle();

    let mut cells = Vec::new();
    for cycle in 0..8usize {
        let held = OwnedMutableState::with_runtime(cycle, handle.clone());
        let holder = OwnedMutableState::with_runtime(None, handle.clone());
        holder.set(Some(held));
        drop(holder);
        cells.push(handle.state_arena_debug_stats().cells_len);
    }

    assert!(
        cells.iter().all(|len| *len == cells[0]),
        "a state written from the global snapshot kept its value alive after its owner dropped it: {cells:?}"
    );
}

#[test]
fn test_assign_value_with_vec() {
    let source = StateRecord::new(SnapshotId(10), vec![1, 2, 3, 4, 5]);
    let target = StateRecord::new(SnapshotId(20), Vec::<i32>::new());

    target.assign_value(&source).expect("copy vec value");

    target.with_value(|val: &Vec<i32>| {
        assert_eq!(val, &vec![1, 2, 3, 4, 5]);
    });

    drop(source.replace_value(vec![10, 20]));
    target.with_value(|val: &Vec<i32>| {
        assert_eq!(val, &vec![1, 2, 3, 4, 5]);
    });
}
