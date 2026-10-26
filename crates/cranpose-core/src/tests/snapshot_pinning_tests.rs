use super::*;

#[test]
fn test_invalid_handle() {
    let handle = PinHandle::INVALID;
    assert!(!handle.is_valid());
    assert_eq!(handle.0, 0);
}

#[test]
fn test_valid_handle() {
    reset_pinning_table();
    let invalid = SnapshotIdSet::new().set(SnapshotId(10));
    let handle = track_pinning(SnapshotId(20), &invalid);
    assert!(handle.is_valid());
    assert!(handle.0 > 0);
}

#[test]
fn test_track_and_release() {
    reset_pinning_table();

    let invalid = SnapshotIdSet::new().set(SnapshotId(10));
    let handle = track_pinning(SnapshotId(20), &invalid);

    assert_eq!(pin_count(), 1);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(10)));

    release_pinning(handle);
    assert_eq!(pin_count(), 0);
    assert_eq!(lowest_pinned_snapshot(), None);
}

#[test]
fn test_multiple_pins() {
    reset_pinning_table();

    let invalid1 = SnapshotIdSet::new().set(SnapshotId(10));
    let handle1 = track_pinning(SnapshotId(20), &invalid1);

    let invalid2 = SnapshotIdSet::new().set(SnapshotId(5)).set(SnapshotId(15));
    let handle2 = track_pinning(SnapshotId(30), &invalid2);

    assert_eq!(pin_count(), 2);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(5)));

    release_pinning(handle1);
    assert_eq!(pin_count(), 1);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(5)));

    release_pinning(handle2);
    assert_eq!(pin_count(), 0);
    assert_eq!(lowest_pinned_snapshot(), None);
}

#[test]
fn test_duplicate_pins() {
    reset_pinning_table();

    let invalid = SnapshotIdSet::new().set(SnapshotId(10));
    let handle1 = track_pinning(SnapshotId(20), &invalid);
    let handle2 = track_pinning(SnapshotId(25), &invalid);

    assert_eq!(pin_count(), 2);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(10)));

    release_pinning(handle1);
    assert_eq!(pin_count(), 1);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(10)));

    release_pinning(handle2);
    assert_eq!(pin_count(), 0);
    assert_eq!(lowest_pinned_snapshot(), None);
}

#[test]
fn test_pin_ordering() {
    reset_pinning_table();

    let invalid1 = SnapshotIdSet::new().set(SnapshotId(30));
    let _handle1 = track_pinning(SnapshotId(40), &invalid1);

    let invalid2 = SnapshotIdSet::new().set(SnapshotId(10));
    let _handle2 = track_pinning(SnapshotId(20), &invalid2);

    let invalid3 = SnapshotIdSet::new().set(SnapshotId(20));
    let _handle3 = track_pinning(SnapshotId(30), &invalid3);

    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(10)));
}

#[test]
fn test_release_invalid_handle() {
    reset_pinning_table();

    release_pinning(PinHandle::INVALID);
    assert_eq!(pin_count(), 0);
}

#[test]
fn test_empty_invalid_set() {
    reset_pinning_table();

    let invalid = SnapshotIdSet::new();
    let handle = track_pinning(SnapshotId(100), &invalid);

    assert_eq!(pin_count(), 1);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(100)));

    release_pinning(handle);
}

#[test]
fn test_lowest_from_invalid_set() {
    reset_pinning_table();

    let invalid = SnapshotIdSet::new()
        .set(SnapshotId(5))
        .set(SnapshotId(10))
        .set(SnapshotId(15))
        .set(SnapshotId(20));
    let handle = track_pinning(SnapshotId(25), &invalid);

    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(5)));

    release_pinning(handle);
}

#[test]
fn test_concurrent_snapshots() {
    reset_pinning_table();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let invalid = SnapshotIdSet::new().set(SnapshotId(i * 10));
            track_pinning(SnapshotId(i * 10 + 5), &invalid)
        })
        .collect();

    assert_eq!(pin_count(), 10);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(0)));

    for handle in handles {
        release_pinning(handle);
    }

    assert_eq!(pin_count(), 0);
    assert_eq!(lowest_pinned_snapshot(), None);
}

#[test]
fn test_heap_handle_based_removal() {
    reset_pinning_table();

    let invalid1 = SnapshotIdSet::new().set(SnapshotId(42));
    let invalid2 = SnapshotIdSet::new().set(SnapshotId(17));
    let invalid3 = SnapshotIdSet::new().set(SnapshotId(99));

    let h1 = track_pinning(SnapshotId(50), &invalid1);
    let h2 = track_pinning(SnapshotId(25), &invalid2);
    let h3 = track_pinning(SnapshotId(100), &invalid3);

    assert_eq!(pin_count(), 3);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(17)));

    release_pinning(h1);
    assert_eq!(pin_count(), 2);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(17)));

    release_pinning(h2);
    assert_eq!(pin_count(), 1);
    assert_eq!(lowest_pinned_snapshot(), Some(SnapshotId(99)));

    release_pinning(h3);
    assert!(pin_count() == 0);
}
