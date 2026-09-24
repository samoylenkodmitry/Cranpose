use std::sync::Arc;

use super::*;
use crate::{
    snapshot_v2::runtime::TestRuntimeGuard,
    state::{NeverEqual, SnapshotMutableState, StateObject},
};

fn reset_runtime() -> TestRuntimeGuard {
    reset_runtime_for_tests()
}

fn new_state(initial: i32) -> Arc<SnapshotMutableState<i32>> {
    SnapshotMutableState::new_in_arc(initial, Arc::new(NeverEqual))
}

struct MockStateObject;

fn mock_state_record() -> Rc<crate::state::StateRecord> {
    crate::state::StateRecord::new(crate::state::PREEXISTING_SNAPSHOT_ID, (), None)
}

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

struct MissingParentReadableStateObject {
    record: Rc<crate::state::StateRecord>,
}

impl MissingParentReadableStateObject {
    fn new(writer_id: SnapshotId) -> Self {
        Self {
            record: crate::state::StateRecord::new(writer_id, (), None),
        }
    }
}

impl StateObject for MissingParentReadableStateObject {
    fn object_id(&self) -> crate::state::ObjectId {
        crate::state::ObjectId(10_001)
    }

    fn first_record(&self) -> Rc<crate::state::StateRecord> {
        Rc::clone(&self.record)
    }

    fn try_readable_record(
        &self,
        _: SnapshotId,
        _: &SnapshotIdSet,
    ) -> Option<Rc<crate::state::StateRecord>> {
        None
    }

    fn readable_record(&self, _: SnapshotId, _: &SnapshotIdSet) -> Rc<crate::state::StateRecord> {
        panic!("apply must use a fallible parent readable-record lookup")
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
fn test_mutable_snapshot_creation() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    assert_eq!(snapshot.snapshot_id(), 1);
    assert!(!snapshot.read_only());
    assert!(!snapshot.is_disposed());
    assert!(!snapshot.applied.get());
}

#[test]
fn test_mutable_snapshot_no_pending_changes_initially() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    assert!(!snapshot.has_pending_changes());
}

#[test]
fn test_mutable_snapshot_enter() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);

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
fn test_mutable_snapshot_read_observer() {
    let _guard = reset_runtime();
    use std::sync::{Arc as StdArc, Mutex};

    let read_count = StdArc::new(Mutex::new(0));
    let read_count_clone = read_count.clone();

    let observer = Arc::new(move |_: &dyn StateObject| {
        *read_count_clone.lock().unwrap() += 1;
    });

    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), Some(observer), None, 0);
    let mock_state = MockStateObject;

    snapshot.record_read(&mock_state);
    snapshot.record_read(&mock_state);

    assert_eq!(*read_count.lock().unwrap(), 2);
}

#[test]
fn test_mutable_snapshot_write_observer() {
    let _guard = reset_runtime();
    use std::sync::{Arc as StdArc, Mutex};

    let write_count = StdArc::new(Mutex::new(0));
    let write_count_clone = write_count.clone();

    let observer = Arc::new(move |_: &dyn StateObject| {
        *write_count_clone.lock().unwrap() += 1;
    });

    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, Some(observer), 0);
    let mock_state = Arc::new(MockStateObject);

    snapshot.record_write(mock_state.clone());
    snapshot.record_write(mock_state);

    assert_eq!(*write_count.lock().unwrap(), 1);
}

#[test]
fn test_mutable_snapshot_apply_empty() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let result = snapshot.apply();
    assert!(result.is_success());
    assert!(snapshot.applied.get());
}

#[test]
fn mutable_apply_returns_failure_when_parent_readable_record_is_missing() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(7, SnapshotIdSet::new(), None, None, 1);
    let state = Arc::new(MissingParentReadableStateObject::new(
        snapshot.snapshot_id(),
    ));

    snapshot.record_write(state);

    let result = snapshot.apply();

    assert!(result.is_failure());
}

#[test]
fn test_mutable_snapshot_apply_twice_fails() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    snapshot.apply().check();

    let result = snapshot.apply();
    assert!(result.is_failure());
}

#[test]
fn test_mutable_snapshot_nested_readonly() {
    let _guard = reset_runtime();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let nested = parent.take_nested_snapshot(None);

    assert_eq!(nested.snapshot_id(), 1);
    assert!(nested.read_only());
    assert_eq!(parent.nested_count.get(), 1);
}

#[test]
fn test_mutable_snapshot_nested_mutable() {
    let _guard = reset_runtime();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let nested = parent.take_nested_mutable_snapshot(None, None);

    assert!(nested.snapshot_id() > parent.snapshot_id());
    assert!(!nested.read_only());
    assert_eq!(parent.nested_count.get(), 1);
}

#[test]
fn test_mutable_snapshot_nested_mutable_dispose_clears_invalid() {
    let _guard = reset_runtime();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let nested = parent.take_nested_mutable_snapshot(None, None);

    let child_id = nested.snapshot_id();
    assert!(parent.state.invalid.borrow().get(child_id));

    nested.dispose();

    assert_eq!(parent.nested_count.get(), 0);
    assert!(!parent.state.invalid.borrow().get(child_id));
}

#[test]
fn test_mutable_snapshot_nested_dispose() {
    let _guard = reset_runtime();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let nested = parent.take_nested_snapshot(None);

    assert_eq!(parent.nested_count.get(), 1);

    nested.dispose();
    assert_eq!(parent.nested_count.get(), 0);
}

#[test]
#[should_panic(expected = "Snapshot has already been applied")]
fn test_mutable_snapshot_write_after_apply_panics() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    snapshot.apply().check();

    let mock_state = Arc::new(MockStateObject);
    snapshot.record_write(mock_state);
}

#[test]
#[should_panic(expected = "Snapshot has been disposed")]
fn test_mutable_snapshot_write_after_dispose_panics() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    snapshot.dispose();

    let mock_state = Arc::new(MockStateObject);
    snapshot.record_write(mock_state);
}

#[test]
fn test_mutable_snapshot_dispose() {
    let _guard = reset_runtime();
    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    assert!(!snapshot.is_disposed());

    snapshot.dispose();
    assert!(snapshot.is_disposed());
}

#[test]
fn test_mutable_snapshot_apply_observer() {
    let _guard = reset_runtime();
    use std::sync::{Arc as StdArc, Mutex};

    let applied_count = StdArc::new(Mutex::new(0));
    let applied_count_clone = applied_count.clone();

    let observer = Rc::new(
        move |_modified: &[Arc<dyn StateObject>], _snapshot_id: SnapshotId| {
            *applied_count_clone.lock().unwrap() += 1;
        },
    );

    let _handle = register_apply_observer(observer);

    let snapshot = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let state = new_state(0);

    snapshot.enter(|| state.set(10));
    snapshot.apply().check();

    assert_eq!(*applied_count.lock().unwrap(), 1);
}

#[test]
fn test_mutable_conflict_detection_same_object() {
    let _guard = reset_runtime();
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
    let _guard = reset_runtime();
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
