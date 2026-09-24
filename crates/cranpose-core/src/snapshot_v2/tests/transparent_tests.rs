use std::rc::Rc;

use super::*;
use crate::{
    snapshot_v2::runtime::TestRuntimeGuard,
    state::{ObjectId, PREEXISTING_SNAPSHOT_ID, StateObject, StateRecord},
};

fn reset_runtime() -> TestRuntimeGuard {
    reset_runtime_for_tests()
}

fn mock_state_record() -> Rc<StateRecord> {
    StateRecord::new(PREEXISTING_SNAPSHOT_ID, (), None)
}

struct MockState(usize);

impl StateObject for MockState {
    fn object_id(&self) -> ObjectId {
        ObjectId(self.0)
    }

    fn first_record(&self) -> Rc<StateRecord> {
        mock_state_record()
    }

    fn try_readable_record(
        &self,
        snapshot_id: SnapshotId,
        invalid: &SnapshotIdSet,
    ) -> Option<Rc<StateRecord>> {
        Some(self.readable_record(snapshot_id, invalid))
    }

    fn readable_record(
        &self,
        _snapshot_id: SnapshotId,
        _invalid: &SnapshotIdSet,
    ) -> Rc<StateRecord> {
        mock_state_record()
    }

    fn prepend_state_record(&self, _record: Rc<StateRecord>) {}

    fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_transparent_observer_mutable_snapshot() {
    let _guard = reset_runtime();
    let snapshot =
        TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

    assert_eq!(snapshot.snapshot_id(), 1);
    assert!(!snapshot.read_only());
    assert!(snapshot.can_reuse());
}

#[test]
fn test_transparent_observer_mutable_apply() {
    let _guard = reset_runtime();
    let snapshot =
        TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

    let result = snapshot.apply();
    assert!(result.is_success());
}

#[test]
fn test_transparent_observer_snapshot() {
    let _guard = reset_runtime();
    let snapshot = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

    assert_eq!(snapshot.snapshot_id(), 1);
    assert!(snapshot.read_only());
    assert!(snapshot.can_reuse());
}

#[test]
#[should_panic(expected = "Cannot write to a read-only snapshot")]
fn test_transparent_observer_snapshot_write_panics() {
    let _guard = reset_runtime();

    let snapshot = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

    let mock_state = Arc::new(MockState(0));
    snapshot.record_write(mock_state);
}

#[test]
fn transparent_mutable_set_read_observer_replaces_observer() {
    let _guard = reset_runtime();
    let initial_reads = Rc::new(Cell::new(0));
    let replacement_reads = Rc::new(Cell::new(0));
    let snapshot = TransparentObserverMutableSnapshot::new(
        1,
        SnapshotIdSet::new(),
        Some(Arc::new({
            let initial_reads = Rc::clone(&initial_reads);
            move |_| initial_reads.set(initial_reads.get() + 1)
        })),
        None,
        None,
    );

    snapshot.set_read_observer(Some(Arc::new({
        let replacement_reads = Rc::clone(&replacement_reads);
        move |_| replacement_reads.set(replacement_reads.get() + 1)
    })));
    snapshot.record_read(&MockState(1));

    assert_eq!(initial_reads.get(), 0);
    assert_eq!(replacement_reads.get(), 1);
}

#[test]
fn transparent_mutable_set_write_observer_replaces_observer() {
    let _guard = reset_runtime();
    let initial_writes = Rc::new(Cell::new(0));
    let replacement_writes = Rc::new(Cell::new(0));
    let snapshot = TransparentObserverMutableSnapshot::new(
        1,
        SnapshotIdSet::new(),
        None,
        Some(Arc::new({
            let initial_writes = Rc::clone(&initial_writes);
            move |_| initial_writes.set(initial_writes.get() + 1)
        })),
        None,
    );

    snapshot.set_write_observer(Some(Arc::new({
        let replacement_writes = Rc::clone(&replacement_writes);
        move |_| replacement_writes.set(replacement_writes.get() + 1)
    })));
    snapshot.record_write(Arc::new(MockState(2)));

    assert_eq!(initial_writes.get(), 0);
    assert_eq!(replacement_writes.get(), 1);
}

#[test]
fn transparent_mutable_nested_snapshot_inherits_replaced_observers() {
    let _guard = reset_runtime();
    let parent_reads = Rc::new(Cell::new(0));
    let parent_writes = Rc::new(Cell::new(0));
    let snapshot =
        TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);
    snapshot.set_read_observer(Some(Arc::new({
        let parent_reads = Rc::clone(&parent_reads);
        move |_| parent_reads.set(parent_reads.get() + 1)
    })));
    snapshot.set_write_observer(Some(Arc::new({
        let parent_writes = Rc::clone(&parent_writes);
        move |_| parent_writes.set(parent_writes.get() + 1)
    })));

    let nested = snapshot.take_nested_mutable_snapshot(None, None);
    nested.record_read(&MockState(3));
    nested.record_write(Arc::new(MockState(4)));

    assert_eq!(parent_reads.get(), 1);
    assert_eq!(parent_writes.get(), 1);
}

#[test]
fn transparent_readonly_set_read_observer_replaces_observer() {
    let _guard = reset_runtime();
    let initial_reads = Rc::new(Cell::new(0));
    let replacement_reads = Rc::new(Cell::new(0));
    let snapshot = TransparentObserverSnapshot::new(
        1,
        SnapshotIdSet::new(),
        Some(Arc::new({
            let initial_reads = Rc::clone(&initial_reads);
            move |_| initial_reads.set(initial_reads.get() + 1)
        })),
        None,
    );

    snapshot.set_read_observer(Some(Arc::new({
        let replacement_reads = Rc::clone(&replacement_reads);
        move |_| replacement_reads.set(replacement_reads.get() + 1)
    })));
    snapshot.record_read(&MockState(5));

    assert_eq!(initial_reads.get(), 0);
    assert_eq!(replacement_reads.get(), 1);
}

#[test]
fn test_transparent_observer_mutable_nested() {
    let _guard = reset_runtime();
    let parent = TransparentObserverMutableSnapshot::new(1, SnapshotIdSet::new(), None, None, None);

    let nested = parent.take_nested_mutable_snapshot(None, None);
    assert!(nested.snapshot_id() > parent.snapshot_id());
}

#[test]
fn test_transparent_observer_snapshot_nested() {
    let _guard = reset_runtime();
    let parent = TransparentObserverSnapshot::new(1, SnapshotIdSet::new(), None, None);

    let nested = parent.take_nested_snapshot(None);
    assert_eq!(nested.snapshot_id(), parent.snapshot_id());
    assert!(nested.read_only());
}
