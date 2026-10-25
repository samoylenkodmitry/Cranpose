use std::rc::Rc;

use super::*;
use crate::{state::StateObject, tests::state_mock::MockState};

#[test]
fn test_readonly_snapshot_creation() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    assert_eq!(snapshot.snapshot_id(), 1);
    assert!(!snapshot.is_disposed());
}

#[test]
fn test_readonly_snapshot_is_valid() {
    let invalid = SnapshotIdSet::new().set(5);
    let snapshot = ReadonlySnapshot::new(10, invalid, None);

    let any_snapshot = AnySnapshot::Readonly(snapshot);
    assert!(any_snapshot.is_valid(1));
    assert!(any_snapshot.is_valid(10));
    assert!(!any_snapshot.is_valid(5));
    assert!(!any_snapshot.is_valid(11));
}

#[test]
fn test_readonly_snapshot_no_pending_changes() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    assert!(!snapshot.has_pending_changes());
}

#[test]
fn test_readonly_snapshot_enter() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);

    set_current_snapshot(None);
    assert!(current_snapshot().is_none());

    snapshot.enter(|| {
        let current = current_snapshot();
        assert!(current.is_some());
        assert_eq!(current.unwrap().snapshot_id(), 1);
    });

    assert!(current_snapshot().is_none());
}

#[test]
fn test_readonly_snapshot_enter_restores_previous() {
    let snapshot1 = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    let snapshot2 = ReadonlySnapshot::new(2, SnapshotIdSet::new(), None);

    snapshot1.enter(|| {
        snapshot2.enter(|| {
            let current = current_snapshot();
            assert_eq!(current.unwrap().snapshot_id(), 2);
        });

        let current = current_snapshot();
        assert_eq!(current.unwrap().snapshot_id(), 1);
    });
}

#[test]
fn test_readonly_snapshot_nested() {
    let parent = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    let nested = parent.take_nested_snapshot(None);

    assert_eq!(nested.snapshot_id(), 1);
}

#[test]
fn test_readonly_snapshot_read_observer() {
    use std::{rc::Rc as ObserverRef, sync::Mutex};

    let read_count = ObserverRef::new(Mutex::new(0));
    let read_count_clone = Rc::clone(&read_count);

    let observer = Rc::new(move |_: &dyn StateObject| {
        *read_count_clone.lock().unwrap() += 1;
    });

    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), Some(observer));
    let mock_state = MockState::with_id(0);

    snapshot.record_read(&mock_state);
    snapshot.record_read(&mock_state);

    assert_eq!(*read_count.lock().unwrap(), 2);
}

#[test]
fn test_readonly_snapshot_nested_with_observer() {
    use std::{rc::Rc as ObserverRef, sync::Mutex};

    let parent_reads = ObserverRef::new(Mutex::new(0));
    let parent_reads_clone = Rc::clone(&parent_reads);
    let parent_observer = Rc::new(move |_: &dyn StateObject| {
        *parent_reads_clone.lock().unwrap() += 1;
    });

    let nested_reads = ObserverRef::new(Mutex::new(0));
    let nested_reads_clone = Rc::clone(&nested_reads);
    let nested_observer = Rc::new(move |_: &dyn StateObject| {
        *nested_reads_clone.lock().unwrap() += 1;
    });

    let parent = ReadonlySnapshot::new(1, SnapshotIdSet::new(), Some(parent_observer));
    let nested = parent.take_nested_snapshot(Some(nested_observer));

    let mock_state = MockState::with_id(0);

    nested.record_read(&mock_state);

    assert_eq!(*parent_reads.lock().unwrap(), 1);
    assert_eq!(*nested_reads.lock().unwrap(), 1);
}

#[test]
#[should_panic(expected = "Cannot write to a read-only snapshot")]
fn test_readonly_snapshot_write_panics() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    let mock_state = Rc::new(MockState::with_id(0));
    snapshot.record_write(mock_state);
}

#[test]
fn test_readonly_snapshot_dispose() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    assert!(!snapshot.is_disposed());

    snapshot.dispose();
    assert!(snapshot.is_disposed());
}
