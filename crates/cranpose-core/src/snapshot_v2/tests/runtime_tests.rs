use super::*;

#[test]
fn test_initial_state_marks_global_snapshot_open() {
    let _guard = reset_runtime_for_tests();
    with_runtime(|runtime| {
        assert_eq!(runtime.global_snapshot_id(), INITIAL_GLOBAL_SNAPSHOT_ID);
        assert!(runtime.open_snapshots().get(INITIAL_GLOBAL_SNAPSHOT_ID));
    });
}

#[test]
fn test_allocate_snapshot_marks_it_open() {
    let _guard = reset_runtime_for_tests();
    let (id, invalid) = allocate_snapshot();
    assert!(invalid.get(INITIAL_GLOBAL_SNAPSHOT_ID));
    assert!(!invalid.get(id));
    with_runtime(|runtime| {
        assert!(runtime.open_snapshots().get(id));
    });
}

#[test]
fn test_close_snapshot_clears_open_flag() {
    let _guard = reset_runtime_for_tests();
    let (id, _) = allocate_snapshot();
    with_runtime(|runtime| {
        assert!(runtime.open_snapshots().get(id));
    });
    close_snapshot(id);
    with_runtime(|runtime| {
        assert!(!runtime.open_snapshots().get(id));
    });
}

#[test]
fn test_advance_global_snapshot_updates_open_set() {
    let _guard = reset_runtime_for_tests();
    let new_id = INITIAL_GLOBAL_SNAPSHOT_ID + 1;
    let open = advance_global_snapshot(new_id);
    assert!(open.get(new_id));
    assert!(!open.get(INITIAL_GLOBAL_SNAPSHOT_ID));
    with_runtime(|runtime| {
        assert_eq!(runtime.global_snapshot_id(), new_id);
        assert!(runtime.open_snapshots().get(new_id));
    });
}
