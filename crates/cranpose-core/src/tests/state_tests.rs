use super::*;

fn create_record_chain(ids: &[SnapshotId]) -> Rc<StateRecord> {
    let mut head: Option<Rc<StateRecord>> = None;

    for &id in ids.iter().rev() {
        head = Some(StateRecord::new(id, 0i32, head));
    }

    head.expect("create_record_chain called with empty ids")
}

struct ManualState {
    head: Rc<StateRecord>,
}

impl ManualState {
    fn new(head: Rc<StateRecord>) -> Self {
        Self { head }
    }
}

impl StateObject for ManualState {
    fn object_id(&self) -> ObjectId {
        ObjectId(999)
    }

    fn first_record(&self) -> Rc<StateRecord> {
        Rc::clone(&self.head)
    }

    fn try_readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Option<Rc<StateRecord>> {
        Some(Rc::clone(&self.head))
    }

    fn readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Rc<StateRecord> {
        Rc::clone(&self.head)
    }

    fn prepend_state_record(&self, _: Rc<StateRecord>) {}

    fn promote_record(&self, _: SnapshotId) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn poison_mutex<T>(mutex: &Mutex<T>) {
    let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        panic!("poison snapshot state mutex for recovery test");
    }));

    assert!(poison_result.is_err());
}

#[test]
fn observation_leases_drive_subscriber_liveness() {
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let notifications = Rc::new(Cell::new(0));
    let notifications_for_callback = Rc::clone(&notifications);
    let callback: Rc<dyn Fn()> = Rc::new(move || {
        notifications_for_callback.set(notifications_for_callback.get() + 1);
    });
    state.subscriber_callback(callback.clone(), false);

    let first = StateObject::observation_lease(&*state).expect("first observation lease");
    let second = StateObject::observation_lease(&*state).expect("second observation lease");
    assert!(state.has_subscribers());
    assert_eq!(notifications.get(), 1);

    drop(first);
    assert!(state.has_subscribers());
    drop(second);
    assert!(!state.has_subscribers());

    let third = StateObject::observation_lease(&*state).expect("third observation lease");
    assert_eq!(notifications.get(), 2);
    drop(third);
}

#[test]
fn observation_leases_preserve_clones_scope_counts_and_state_lifetimes() {
    let first = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let second = SnapshotMutableState::new_in_arc(200i32, Arc::new(NeverEqual));
    let lease = first.observation_lease().unwrap();
    let clone = Rc::clone(&lease);
    assert!(first.has_subscribers());
    assert!(!second.has_subscribers());

    drop(lease);
    assert!(first.has_subscribers());
    assert!(!first.add_scope_observer());
    drop(clone);
    assert!(first.has_subscribers());
    first.remove_scope_observers(1);
    assert!(!first.has_subscribers());

    let lease = second.observation_lease().unwrap();
    let weak = Arc::downgrade(&second);
    drop(second);
    assert!(weak.upgrade().is_none());
    drop(lease);
    assert!(!first.has_subscribers());
}

#[test]
fn snapshot_mutable_state_recovers_poisoned_weak_self_lock() {
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));

    poison_mutex(&state.weak_self);

    assert_eq!(state.get(), 100);
    assert!(state.set(101));
    assert_eq!(state.get(), 101);
}

#[test]
fn snapshot_mutable_state_recovers_poisoned_apply_observer_lock() {
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let calls = Rc::new(Cell::new(0usize));
    let observed_calls = Rc::clone(&calls);

    poison_mutex(&state.apply_observers);

    state.add_apply_observer(Box::new(move || {
        observed_calls.set(observed_calls.get() + 1);
    }));
    state.notify_applied();

    assert_eq!(calls.get(), 1);
}

#[test]
fn snapshot_mutable_state_promote_missing_record_returns_error() {
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let missing_snapshot = usize::MAX - 17;

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
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let child_snapshot = usize::MAX - 31;
    let wrong_record = StateRecord::new(child_snapshot, "wrong type", None);
    StateObject::prepend_state_record(&*state, wrong_record);

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
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let merged = StateRecord::new(usize::MAX - 43, "wrong type", None);

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
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let previous = StateRecord::new(usize::MAX - 51, 1i32, None);
    let current = StateRecord::new(usize::MAX - 52, "wrong type", None);
    let applied = StateRecord::new(usize::MAX - 53, 2i32, None);

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
    let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
    let invalid_rec = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, Some(tail));
    let head = StateRecord::new(10, 0i32, Some(invalid_rec.clone()));

    let result = used_locked(&head);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_used_locked_finds_obscured_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let pin_handle = crate::snapshot_pinning::track_pinning(10, &SnapshotIdSet::EMPTY);

    let oldest = StateRecord::new(2, 0i32, None);
    let newer = StateRecord::new(5, 0i32, Some(oldest.clone()));
    let head = StateRecord::new(100, 0i32, Some(newer));

    let result = used_locked(&head);

    assert!(result.is_some());
    let reused = result.unwrap();
    assert_eq!(
        reused.snapshot_id(),
        2,
        "Should return the oldest obscured record"
    );

    crate::snapshot_pinning::release_pinning(pin_handle);
}

#[test]
fn test_used_locked_no_reusable_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let high_id = allocate_record_id() + 1000;
    let head = create_record_chain(&[high_id, high_id + 1, high_id + 2]);

    let result = used_locked(&head);
    assert!(
        result.is_none(),
        "Should find no reusable records when all are recent"
    );
}

#[test]
fn test_used_locked_single_old_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let old = StateRecord::new(2, 0i32, None);
    let head = StateRecord::new(100, 0i32, Some(old));

    let result = used_locked(&head);
    assert!(result.is_none(), "Single old record should not be reused");
}

#[test]
fn test_readable_record_for_preexisting() {
    let head = create_record_chain(&[PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::EMPTY;

    let result = readable_record_for(&head, 10, &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
}

#[test]
fn test_readable_record_for_picks_highest_valid() {
    let head = create_record_chain(&[10, 5, PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::EMPTY;

    let result = readable_record_for(&head, 10, &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), 10);

    let result = readable_record_for(&head, 7, &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), 5);
}

#[test]
fn test_new_overwritable_record_locked_reuses_invalid() {
    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));

    let current_head = state.first_record();
    let invalid_rec = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, current_head.next());
    current_head.set_next(Some(invalid_rec.clone()));

    let result = new_overwritable_record_locked(&*state);

    assert!(Rc::ptr_eq(&result, &invalid_rec));
    assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);
}

#[test]
fn test_new_overwritable_record_locked_creates_new() {
    crate::snapshot_pinning::reset_pinning_table();

    let _pin_handle = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

    let state = SnapshotMutableState::new_in_arc(100i32, Arc::new(NeverEqual));
    let old_head = state.first_record();

    let result = new_overwritable_record_locked(&*state);

    assert_eq!(result.snapshot_id(), SNAPSHOT_ID_MAX);

    let new_head = state.first_record();
    assert!(
        Rc::ptr_eq(&new_head, &result),
        "new_head ({:p}) should equal result ({:p})",
        Rc::as_ptr(&new_head),
        Rc::as_ptr(&result)
    );

    assert!(result.next().is_some());
    assert!(Rc::ptr_eq(&result.next().unwrap(), &old_head));
}

#[test]
fn test_writable_record_reuses_invalid_record() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_arc(7i32, Arc::new(NeverEqual));

    let head = state.first_record();
    let invalid = StateRecord::new(INVALID_SNAPSHOT_ID, 0i32, head.next());
    head.set_next(Some(invalid.clone()));

    let snapshot_id = allocate_record_id();
    let result = state.writable_record(snapshot_id, &SnapshotIdSet::EMPTY);

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
    let pin = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

    let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));
    let original_head = state.first_record();
    let preexisting = original_head
        .next()
        .expect("preexisting record should exist for newly created state");

    let snapshot_id = allocate_record_id();
    let result = state.writable_record(snapshot_id, &SnapshotIdSet::EMPTY);

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

    let new_head = state.first_record();
    assert!(
        Rc::ptr_eq(&new_head, &result),
        "Newly created record should become the head of the chain"
    );

    crate::snapshot_pinning::release_pinning(pin);
}

#[test]
fn test_state_record_clear_for_reuse() {
    let record = StateRecord::new(10, 42i32, None);

    record.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    record.clear_for_reuse();

    assert_eq!(record.snapshot_id(), 10);
}

#[test]
fn test_overwrite_unused_records_no_old_records() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));

    let _pin = crate::snapshot_pinning::track_pinning(1, &SnapshotIdSet::EMPTY);

    let should_retain = state.overwrite_unused_records();

    assert!(
        should_retain,
        "Should retain multiple records when none are old enough"
    );

    let mut cursor = Some(state.first_record());
    while let Some(record) = cursor {
        assert_ne!(record.snapshot_id(), INVALID_SNAPSHOT_ID);
        cursor = record.next();
    }
}

#[test]
fn test_overwrite_unused_records_basic_cleanup() {
    crate::snapshot_pinning::reset_pinning_table();

    let rec1 = StateRecord::new(100, 1i32, None);
    let rec2 = StateRecord::new(200, 2i32, Some(rec1.clone()));
    let rec3 = StateRecord::new(300, 3i32, Some(rec2.clone()));

    let test_state = ManualState::new(rec3.clone());

    let _pin = crate::snapshot_pinning::track_pinning(1000, &SnapshotIdSet::EMPTY);

    let result = overwrite_unused_records_locked::<i32>(&test_state);

    assert_eq!(rec3.snapshot_id(), 300);
    assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
    assert_eq!(rec1.snapshot_id(), INVALID_SNAPSHOT_ID);

    assert!(!result);
}

#[test]
fn test_overwrite_unused_records_single_record_only() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));

    let head = state.first_record();
    head.set_next(None);

    let should_retain = state.overwrite_unused_records();

    assert!(!should_retain, "Single record should return false");
}

#[test]
fn snapshot_state_try_get_reports_missing_visible_record_without_panicking() {
    crate::snapshot_pinning::reset_pinning_table();

    let state = SnapshotMutableState::new_in_arc(42i32, Arc::new(NeverEqual));
    let head = state.first_record();
    head.set_snapshot_id(SNAPSHOT_ID_MAX);
    head.set_next(None);

    assert_eq!(state.try_get(), None);
}

#[test]
fn test_overwrite_unused_records_clears_values() {
    crate::snapshot_pinning::reset_pinning_table();

    let tail = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
    let old_rec1 = StateRecord::new(2, 999i32, Some(tail.clone()));
    let old_rec2 = StateRecord::new(3, 888i32, Some(old_rec1.clone()));
    let head = StateRecord::new(150, 42i32, Some(old_rec2.clone()));
    let state = ManualState::new(head.clone());

    old_rec1.with_value(|val: &i32| {
        assert_eq!(*val, 999);
    });

    let _pin = crate::snapshot_pinning::track_pinning(100, &SnapshotIdSet::EMPTY);
    overwrite_unused_records_locked::<i32>(&state);

    assert_eq!(old_rec1.snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_overwrite_unused_records_mixed_old_and_new() {
    crate::snapshot_pinning::reset_pinning_table();

    let preexisting = StateRecord::new(PREEXISTING_SNAPSHOT_ID, 0i32, None);
    let rec2 = StateRecord::new(2, 100i32, Some(preexisting.clone()));
    let rec5 = StateRecord::new(5, 100i32, Some(rec2.clone()));
    let rec50 = StateRecord::new(50, 100i32, Some(rec5.clone()));
    let head = StateRecord::new(120, 100i32, Some(rec50.clone()));
    let state = ManualState::new(head.clone());

    let _pin = crate::snapshot_pinning::track_pinning(40, &SnapshotIdSet::EMPTY);

    let should_retain = overwrite_unused_records_locked::<i32>(&state);
    assert!(should_retain);

    assert_eq!(rec50.snapshot_id(), 50);
    assert_eq!(rec5.snapshot_id(), 5);
    assert_eq!(rec2.snapshot_id(), INVALID_SNAPSHOT_ID);
}

#[test]
fn test_readable_record_for_skips_invalid_set() {
    let head = create_record_chain(&[10, 5, PREEXISTING_SNAPSHOT_ID]);
    let invalid = SnapshotIdSet::new().set(5);

    let result = readable_record_for(&head, 10, &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), 10);

    let result = readable_record_for(&head, 7, &invalid);
    assert!(result.is_some());
    assert_eq!(result.unwrap().snapshot_id(), PREEXISTING_SNAPSHOT_ID);
}

#[test]
fn test_assign_value_copies_int() {
    let source = StateRecord::new(10, 42i32, None);
    let target = StateRecord::new(20, 0i32, None);

    target.assign_value::<i32>(&source).expect("copy int value");

    target.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    source.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });

    assert_eq!(source.snapshot_id(), 10);
    assert_eq!(target.snapshot_id(), 20);
}

#[test]
fn test_assign_value_copies_string() {
    let source = StateRecord::new(10, "hello".to_string(), None);
    let target = StateRecord::new(20, "world".to_string(), None);

    target
        .assign_value::<String>(&source)
        .expect("copy string value");

    target.with_value(|val: &String| {
        assert_eq!(val, "hello");
    });

    source.with_value(|val: &String| {
        assert_eq!(val, "hello");
    });
}

#[test]
fn test_assign_value_reports_cleared_source() {
    let source = StateRecord::new(10, 42i32, None);
    let target = StateRecord::new(20, 0i32, None);

    source.clear_value();

    assert_eq!(
        target.assign_value::<i32>(&source),
        Err(StateRecordValueError::MissingOrWrongType {
            expected: std::any::type_name::<i32>(),
        })
    );
    assert_eq!(target.with_value(|val: &i32| *val), 0);
}

#[test]
fn test_assign_value_overwrites_existing_value() {
    let source = StateRecord::new(10, 100i32, None);
    let target = StateRecord::new(20, 999i32, None);

    target.with_value(|val: &i32| {
        assert_eq!(*val, 999);
    });

    target
        .assign_value::<i32>(&source)
        .expect("overwrite int value");

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

    let source = StateRecord::new(10, Point { x: 1.5, y: 2.5 }, None);
    let target = StateRecord::new(20, Point { x: 0.0, y: 0.0 }, None);

    target
        .assign_value::<Point>(&source)
        .expect("copy point value");

    target.with_value(|val: &Point| {
        assert_eq!(val, &Point { x: 1.5, y: 2.5 });
    });
}

#[test]
fn test_assign_value_self_assignment() {
    let record = StateRecord::new(10, 42i32, None);

    record
        .assign_value::<i32>(&record)
        .expect("self-assign int value");

    record.with_value(|val: &i32| {
        assert_eq!(*val, 42);
    });
}

#[test]
fn event_loop_writes_keep_the_record_chain_bounded() {
    crate::snapshot_pinning::reset_pinning_table();
    let state = SnapshotMutableState::new_in_arc(0.0f32, Arc::new(NeverEqual));

    let mut lens = Vec::new();
    for event in 0..3000usize {
        crate::run_in_mutable_snapshot(|| {
            state.set(event as f32);
        })
        .expect("event snapshot applies");
        let _ = state.get();
        if event % 500 == 499 {
            lens.push(state.record_chain_debug().len());
        }
    }

    let final_len = *lens.last().expect("sampled chain lengths");
    assert!(
        final_len <= 16,
        "record chain grew without bound across event-loop writes: {lens:?}"
    );
}

#[test]
fn test_assign_value_with_vec() {
    let source = StateRecord::new(10, vec![1, 2, 3, 4, 5], None);
    let target = StateRecord::new(20, Vec::<i32>::new(), None);

    target
        .assign_value::<Vec<i32>>(&source)
        .expect("copy vec value");

    target.with_value(|val: &Vec<i32>| {
        assert_eq!(val, &vec![1, 2, 3, 4, 5]);
    });

    source.replace_value(vec![10, 20]);
    target.with_value(|val: &Vec<i32>| {
        assert_eq!(val, &vec![1, 2, 3, 4, 5]);
    });
}
