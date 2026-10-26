use super::*;
use crate::snapshot_v2::runtime::TestRuntimeGuard;

fn reset_runtime() -> TestRuntimeGuard {
    let guard = reset_runtime_for_tests();
    clear_global_snapshot_for_tests();
    guard
}

#[test]
fn test_global_snapshot_creation() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    assert_eq!(snapshot.snapshot_id(), SnapshotId(1));
    assert!(!snapshot.read_only());
    assert!(!snapshot.is_disposed());
}

#[test]
fn test_global_snapshot_get_or_create() {
    let _guard = reset_runtime();

    let snapshot1 = GlobalSnapshot::get_or_create();
    let snapshot2 = GlobalSnapshot::get_or_create();

    assert_eq!(snapshot1.snapshot_id(), snapshot2.snapshot_id());
}

#[test]
fn test_global_snapshot_advance() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    assert_eq!(snapshot.snapshot_id(), SnapshotId(1));

    snapshot.state.id.set(SnapshotId(5));
    assert_eq!(snapshot.snapshot_id(), SnapshotId(5));

    snapshot.state.id.set(SnapshotId(10));
    assert_eq!(snapshot.snapshot_id(), SnapshotId(10));
}

#[test]
fn test_global_snapshot_never_disposed() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    assert!(!snapshot.is_disposed());

    snapshot.dispose();
    assert!(!snapshot.is_disposed());
}

#[test]
fn test_global_snapshot_apply_always_succeeds() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    let result = snapshot.apply();
    assert!(result.is_success());
}

#[test]
fn test_global_snapshot_nested() {
    let _guard = reset_runtime();
    let global = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    let nested = global.take_nested_snapshot(None);

    // The snapshot takes an id of its own, and the global moves past it.
    assert!(nested.snapshot_id() > SnapshotId(1));
    assert!(nested.snapshot_id() < global.snapshot_id());
    assert!(nested.read_only());
    assert_eq!(global.nested_count.get(), 0);
}

#[test]
fn test_global_snapshot_nested_mutable() {
    let _guard = reset_runtime();
    let global = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    let nested = global.take_nested_mutable_snapshot(None, None);

    assert!(nested.snapshot_id() < global.snapshot_id());
    assert!(!nested.read_only());
}

#[test]
fn test_global_snapshot_nested_mutable_dispose_clears_invalid() {
    let _guard = reset_runtime();
    let global = GlobalSnapshot::get_or_create();
    let nested = global.take_nested_mutable_snapshot(None, None);
    let child_id = nested.snapshot_id();

    assert!(global.state.invalid.borrow().get(child_id));
    assert_eq!(global.nested_count.get(), 1);

    nested.dispose();

    assert_eq!(global.nested_count.get(), 0);
    assert!(!global.state.invalid.borrow().get(child_id));
}

#[test]
fn test_advance_global_snapshot_function() {
    let _guard = reset_runtime();

    let initial_id = global_snapshot_id();

    advance_global_snapshot(SnapshotId::new(initial_id.index() + 10));
    assert_eq!(
        global_snapshot_id(),
        SnapshotId::new(initial_id.index() + 10)
    );

    advance_global_snapshot(SnapshotId::new(initial_id.index() + 20));
    assert_eq!(
        global_snapshot_id(),
        SnapshotId::new(initial_id.index() + 20)
    );
}

#[test]
fn test_global_snapshot_has_no_pending_changes_initially() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());
    assert!(!snapshot.has_pending_changes());
}

#[test]
fn test_global_snapshot_enter() {
    let _guard = reset_runtime();
    let snapshot = GlobalSnapshot::new(SnapshotId(1), SnapshotIdSet::new());

    set_current_snapshot(None);
    snapshot.enter(|| {
        let current = current_snapshot();
        assert!(current.is_some());
    });
}
