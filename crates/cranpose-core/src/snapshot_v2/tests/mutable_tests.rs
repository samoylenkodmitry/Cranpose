use std::rc::Rc;

use super::*;
use crate::{
    state::{NeverEqual, SnapshotMutableState, StateObject},
    tests::state_mock::MockState,
};

fn new_state(initial: i32) -> Rc<SnapshotMutableState<i32>> {
    SnapshotMutableState::new_in_rc(initial, Rc::new(NeverEqual))
}

// A root mutable snapshot with id 1 and no invalid ids.
fn root_snapshot() -> Rc<MutableSnapshot> {
    MutableSnapshot::new(
        SnapshotId(1),
        SnapshotIdSet::new(),
        None,
        None,
        SnapshotId(0),
    )
}

#[test]
fn test_mutable_snapshot_creation() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    assert_eq!(snapshot.snapshot_id(), SnapshotId(1));
    assert!(!snapshot.read_only());
    assert!(!snapshot.is_disposed());
    assert!(!snapshot.applied.get());
}

#[test]
fn test_mutable_snapshot_no_pending_changes_initially() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    assert!(!snapshot.has_pending_changes());
}

#[test]
fn test_mutable_snapshot_enter() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();

    set_current_snapshot(None);
    assert!(current_snapshot().is_none());

    snapshot.enter(|| {
        let current = current_snapshot();
        assert!(current.is_some());
        assert_eq!(current.unwrap().snapshot_id(), SnapshotId(1));
    });

    assert!(current_snapshot().is_none());
}

#[test]
fn test_mutable_snapshot_read_observer() {
    let _guard = reset_runtime_for_tests();
    use std::{rc::Rc as ObserverRef, sync::Mutex};

    let read_count = ObserverRef::new(Mutex::new(0));
    let read_count_clone = Rc::clone(&read_count);

    let observer = Rc::new(move |_: &dyn StateObject| {
        *read_count_clone.lock().unwrap() += 1;
    });

    let snapshot = MutableSnapshot::new(
        SnapshotId(1),
        SnapshotIdSet::new(),
        Some(observer),
        None,
        SnapshotId(0),
    );
    let mock_state = MockState::with_id(0);

    snapshot.record_read(&mock_state);
    snapshot.record_read(&mock_state);

    assert_eq!(*read_count.lock().unwrap(), 2);
}

#[test]
fn test_mutable_snapshot_write_observer() {
    let _guard = reset_runtime_for_tests();
    use std::{rc::Rc as ObserverRef, sync::Mutex};

    let write_count = ObserverRef::new(Mutex::new(0));
    let write_count_clone = Rc::clone(&write_count);

    let observer = Rc::new(move |_: &dyn StateObject| {
        *write_count_clone.lock().unwrap() += 1;
    });

    let snapshot = MutableSnapshot::new(
        SnapshotId(1),
        SnapshotIdSet::new(),
        None,
        Some(observer),
        SnapshotId(0),
    );
    let mock_state = Rc::new(MockState::with_id(0));

    snapshot.record_write(Rc::<MockState>::clone(&mock_state));
    snapshot.record_write(mock_state);

    assert_eq!(*write_count.lock().unwrap(), 1);
}

#[test]
fn test_mutable_snapshot_apply_empty() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    let result = snapshot.apply();
    assert!(result.is_success());
    assert!(snapshot.applied.get());
}

#[test]
fn mutable_apply_returns_failure_when_parent_readable_record_is_missing() {
    let _guard = reset_runtime_for_tests();
    let snapshot = MutableSnapshot::new(
        SnapshotId(7),
        SnapshotIdSet::new(),
        None,
        None,
        SnapshotId(1),
    );
    let state = Rc::new(MockState::unreadable(10_001, snapshot.snapshot_id()));

    snapshot.record_write(state);

    let result = snapshot.apply();

    assert!(result.is_failure());
}

#[test]
fn test_mutable_snapshot_apply_twice_fails() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    snapshot.apply().check();

    let result = snapshot.apply();
    assert!(result.is_failure());
}

#[test]
fn test_mutable_snapshot_nested_readonly() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let nested = parent.take_nested_snapshot(None);

    assert_eq!(nested.snapshot_id(), SnapshotId(1));
    assert!(nested.read_only());
    assert_eq!(parent.nested_count.get(), 1);
}

#[test]
fn test_mutable_snapshot_nested_mutable() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let nested = parent.take_nested_mutable_snapshot(None, None);

    assert!(nested.snapshot_id() > parent.snapshot_id());
    assert!(!nested.read_only());
    assert_eq!(parent.nested_count.get(), 1);
}

#[test]
fn test_mutable_snapshot_nested_mutable_dispose_clears_invalid() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let nested = parent.take_nested_mutable_snapshot(None, None);

    let child_id = nested.snapshot_id();
    assert!(parent.state.invalid.borrow().get(child_id));

    nested.dispose();

    assert_eq!(parent.nested_count.get(), 0);
    assert!(!parent.state.invalid.borrow().get(child_id));
}

#[test]
fn test_mutable_snapshot_nested_dispose() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let nested = parent.take_nested_snapshot(None);

    assert_eq!(parent.nested_count.get(), 1);

    nested.dispose();
    assert_eq!(parent.nested_count.get(), 0);
}

#[test]
#[should_panic(expected = "Snapshot has already been applied")]
fn test_mutable_snapshot_write_after_apply_panics() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    snapshot.apply().check();

    let mock_state = Rc::new(MockState::with_id(0));
    snapshot.record_write(mock_state);
}

#[test]
#[should_panic(expected = "Snapshot has been disposed")]
fn test_mutable_snapshot_write_after_dispose_panics() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    snapshot.dispose();

    let mock_state = Rc::new(MockState::with_id(0));
    snapshot.record_write(mock_state);
}

#[test]
fn test_mutable_snapshot_dispose() {
    let _guard = reset_runtime_for_tests();
    let snapshot = root_snapshot();
    assert!(!snapshot.is_disposed());

    snapshot.dispose();
    assert!(snapshot.is_disposed());
}

#[test]
fn test_mutable_snapshot_apply_observer() {
    let _guard = reset_runtime_for_tests();
    use std::{rc::Rc as ObserverRef, sync::Mutex};

    let applied_count = ObserverRef::new(Mutex::new(0));
    let applied_count_clone = Rc::clone(&applied_count);

    let observer = Rc::new(
        move |_modified: &[Rc<dyn StateObject>], _snapshot_id: SnapshotId| {
            *applied_count_clone.lock().unwrap() += 1;
        },
    );

    let _handle = register_apply_observer(observer);

    let snapshot = root_snapshot();
    let state = new_state(0);

    snapshot.enter(|| state.set(10));
    snapshot.apply().check();

    assert_eq!(*applied_count.lock().unwrap(), 1);
}

#[test]
fn test_mutable_conflict_detection_same_object() {
    let _guard = reset_runtime_for_tests();
    let global = GlobalSnapshot::get_or_create();
    let state = new_state(0);

    let s1 = global.take_nested_mutable_snapshot(None, None);
    s1.enter(|| state.set(1));

    let s2 = global.take_nested_mutable_snapshot(None, None);
    s2.enter(|| state.set(2));

    assert!(s1.apply().is_success());
    assert!(s2.apply().is_failure());
}

#[test]
fn test_mutable_no_conflict_different_objects() {
    let _guard = reset_runtime_for_tests();
    let global = GlobalSnapshot::get_or_create();
    let state1 = new_state(0);
    let state2 = new_state(0);

    let s1 = global.take_nested_mutable_snapshot(None, None);
    s1.enter(|| state1.set(10));

    let s2 = global.take_nested_mutable_snapshot(None, None);
    s2.enter(|| state2.set(20));

    assert!(s1.apply().is_success());
    assert!(s2.apply().is_success());
}
