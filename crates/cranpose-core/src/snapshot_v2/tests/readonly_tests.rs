use std::rc::Rc;

use super::*;
use crate::state::{PREEXISTING_SNAPSHOT_ID, StateObject};

fn mock_state_record() -> Rc<crate::state::StateRecord> {
    crate::state::StateRecord::new(PREEXISTING_SNAPSHOT_ID, (), None)
}

struct MockStateObject;

impl StateObject for MockStateObject {
    fn object_id(&self) -> crate::state::ObjectId {
        crate::state::ObjectId(0)
    }

    fn first_record(&self) -> Rc<crate::state::StateRecord> {
        mock_state_record()
    }

    fn try_readable_record(
        &self,
        snapshot_id: crate::snapshot_id_set::SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<crate::state::StateRecord>> {
        Some(self.readable_record(snapshot_id, invalid))
    }

    fn readable_record(
        &self,
        _snapshot_id: crate::snapshot_id_set::SnapshotId,
        _invalid: &SnapshotIdSet,
    ) -> Rc<crate::state::StateRecord> {
        mock_state_record()
    }

    fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {}

    fn promote_record(
        &self,
        _child_id: crate::snapshot_id_set::SnapshotId,
    ) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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
    use std::sync::{Arc as StdArc, Mutex};

    let read_count = StdArc::new(Mutex::new(0));
    let read_count_clone = read_count.clone();

    let observer = Arc::new(move |_: &dyn StateObject| {
        *read_count_clone.lock().unwrap() += 1;
    });

    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), Some(observer));
    let mock_state = MockStateObject;

    snapshot.record_read(&mock_state);
    snapshot.record_read(&mock_state);

    assert_eq!(*read_count.lock().unwrap(), 2);
}

#[test]
fn test_readonly_snapshot_nested_with_observer() {
    use std::sync::{Arc as StdArc, Mutex};

    let parent_reads = StdArc::new(Mutex::new(0));
    let parent_reads_clone = parent_reads.clone();
    let parent_observer = Arc::new(move |_: &dyn StateObject| {
        *parent_reads_clone.lock().unwrap() += 1;
    });

    let nested_reads = StdArc::new(Mutex::new(0));
    let nested_reads_clone = nested_reads.clone();
    let nested_observer = Arc::new(move |_: &dyn StateObject| {
        *nested_reads_clone.lock().unwrap() += 1;
    });

    let parent = ReadonlySnapshot::new(1, SnapshotIdSet::new(), Some(parent_observer));
    let nested = parent.take_nested_snapshot(Some(nested_observer));

    let mock_state = MockStateObject;

    nested.record_read(&mock_state);

    assert_eq!(*parent_reads.lock().unwrap(), 1);
    assert_eq!(*nested_reads.lock().unwrap(), 1);
}

#[test]
#[should_panic(expected = "Cannot write to a read-only snapshot")]
fn test_readonly_snapshot_write_panics() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    let mock_state = Arc::new(MockStateObject);
    snapshot.record_write(mock_state);
}

#[test]
fn test_readonly_snapshot_dispose() {
    let snapshot = ReadonlySnapshot::new(1, SnapshotIdSet::new(), None);
    assert!(!snapshot.is_disposed());

    snapshot.dispose();
    assert!(snapshot.is_disposed());
}
