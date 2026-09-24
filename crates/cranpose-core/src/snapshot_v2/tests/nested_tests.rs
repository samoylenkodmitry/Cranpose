use std::rc::Rc;

use super::*;

fn mock_state_record() -> Rc<crate::state::StateRecord> {
    crate::state::StateRecord::new(crate::state::PREEXISTING_SNAPSHOT_ID, (), None)
}

struct TestObj {
    id: crate::state::ObjectId,
}

impl StateObject for TestObj {
    fn object_id(&self) -> crate::state::ObjectId {
        self.id
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
fn test_nested_readonly_snapshot() {
    let _guard = reset_runtime_for_tests();
    let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Arc::downgrade(&parent);

    let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

    assert_eq!(nested.snapshot_id(), 1);
    assert!(nested.read_only());
    assert!(!nested.is_disposed());
}

#[test]
fn test_nested_readonly_snapshot_root() {
    let _guard = reset_runtime_for_tests();
    let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Arc::downgrade(&parent);

    let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

    let root = nested.root_nested_readonly();
    assert_eq!(root.snapshot_id(), 1);
}

#[test]
fn test_nested_readonly_dispose() {
    let _guard = reset_runtime_for_tests();
    let parent = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Arc::downgrade(&parent);

    let nested = NestedReadonlySnapshot::new(1, SnapshotIdSet::new(), None, parent_weak);

    nested.dispose();
    assert!(nested.is_disposed());
}

#[test]
fn test_nested_mutable_snapshot() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let parent_weak = Arc::downgrade(&parent);

    let nested =
        NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

    assert_eq!(nested.snapshot_id(), 2);
    assert!(!nested.read_only());
    assert!(!nested.is_disposed());
}

#[test]
fn test_nested_mutable_apply() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let parent_weak = Arc::downgrade(&parent);

    let nested =
        NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

    let result = nested.apply();
    assert!(result.is_success());
    assert!(nested.applied.get());
}

#[test]
fn test_nested_merge_sets_parent_pending_changes() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let child = parent.take_nested_mutable_snapshot(None, None);

    let obj = Arc::new(TestObj {
        id: crate::state::ObjectId(100),
    });
    child.record_write(obj);
    assert!(!parent.has_pending_changes());
    child.apply().check();
    assert!(parent.has_pending_changes());
}

#[test]
fn test_nested_conflict_with_parent_same_object() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let child = parent.take_nested_mutable_snapshot(None, None);

    let obj = Arc::new(TestObj {
        id: crate::state::ObjectId(200),
    });
    parent.record_write(obj.clone());
    child.record_write(obj);

    let result = child.apply();
    assert!(result.is_failure());
}

#[test]
fn test_nested_mutable_apply_twice_fails() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let parent_weak = Arc::downgrade(&parent);

    let nested =
        NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

    nested.apply().check();
    let result = nested.apply();
    assert!(result.is_failure());
}

#[test]
fn test_nested_mutable_dispose() {
    let _guard = reset_runtime_for_tests();
    let parent = MutableSnapshot::new(1, SnapshotIdSet::new(), None, None, 0);
    let parent_weak = Arc::downgrade(&parent);

    let nested =
        NestedMutableSnapshot::new(2, SnapshotIdSet::new().set(1), None, None, parent_weak, 1);

    nested.dispose();
    assert!(nested.is_disposed());
}
