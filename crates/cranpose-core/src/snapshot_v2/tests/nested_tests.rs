use std::rc::{Rc, Weak};

use super::*;
use crate::tests::state_mock::MockState;

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

// A nested mutable snapshot with id 2 under `parent`, which has id 1.
fn nested_snapshot(parent: Weak<MutableSnapshot>) -> Rc<NestedMutableSnapshot> {
    NestedMutableSnapshot::new(
        SnapshotId(2),
        SnapshotIdSet::new().set(SnapshotId(1)),
        None,
        None,
        parent,
        SnapshotId(1),
    )
}

#[test]
fn test_nested_readonly_snapshot() {
    let _guard = reset_runtime_for_tests();
    let parent =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Rc::downgrade(&parent);

    let nested =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, parent_weak);

    assert_eq!(nested.snapshot_id(), SnapshotId(1));
    assert!(nested.read_only());
    assert!(!nested.is_disposed());
}

#[test]
fn test_nested_readonly_snapshot_root() {
    let _guard = reset_runtime_for_tests();
    let parent =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Rc::downgrade(&parent);

    let nested =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, parent_weak);

    let root = nested.root_nested_readonly();
    assert_eq!(root.snapshot_id(), SnapshotId(1));
}

#[test]
fn test_nested_readonly_dispose() {
    let _guard = reset_runtime_for_tests();
    let parent =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, Weak::new());
    let parent_weak = Rc::downgrade(&parent);

    let nested =
        NestedReadonlySnapshot::new(SnapshotId(1), SnapshotIdSet::new(), None, parent_weak);

    nested.dispose();
    assert!(nested.is_disposed());
}

#[test]
fn test_nested_mutable_snapshot() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let parent_weak = Rc::downgrade(&parent);

    let nested = nested_snapshot(parent_weak);

    assert_eq!(nested.snapshot_id(), SnapshotId(2));
    assert!(!nested.read_only());
    assert!(!nested.is_disposed());
}

#[test]
fn test_nested_mutable_apply() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let parent_weak = Rc::downgrade(&parent);

    let nested = nested_snapshot(parent_weak);

    let result = nested.apply();
    assert!(result.is_success());
    assert!(nested.applied.get());
}

#[test]
fn test_nested_merge_sets_parent_pending_changes() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let child = parent.take_nested_mutable_snapshot(None, None);

    let obj = Rc::new(MockState::with_id(100));
    child.record_write(obj);
    assert!(!parent.has_pending_changes());
    child.apply().check();
    assert!(parent.has_pending_changes());
}

#[test]
fn test_nested_conflict_with_parent_same_object() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let child = parent.take_nested_mutable_snapshot(None, None);

    let obj = Rc::new(MockState::with_id(200));
    parent.record_write(Rc::<MockState>::clone(&obj));
    child.record_write(obj);

    let result = child.apply();
    assert!(result.is_failure());
}

#[test]
fn test_nested_mutable_apply_twice_fails() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let parent_weak = Rc::downgrade(&parent);

    let nested = nested_snapshot(parent_weak);

    nested.apply().check();
    let result = nested.apply();
    assert!(result.is_failure());
}

#[test]
fn test_nested_mutable_dispose() {
    let _guard = reset_runtime_for_tests();
    let parent = root_snapshot();
    let parent_weak = Rc::downgrade(&parent);

    let nested = nested_snapshot(parent_weak);

    nested.dispose();
    assert!(nested.is_disposed());
}
