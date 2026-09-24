use super::*;

#[test]
fn test_empty_set() {
    let set = SnapshotIdSet::EMPTY;
    assert!(set.is_empty());
    assert!(!set.get(0));
    assert!(!set.get(100));
}

#[test]
fn test_set_and_get_lower_range() {
    let set = SnapshotIdSet::new();
    let set = set.set(0);
    assert!(set.get(0));
    assert!(!set.get(1));

    let set = set.set(63);
    assert!(set.get(0));
    assert!(set.get(63));
    assert!(!set.get(64));
}

#[test]
fn test_set_and_get_upper_range() {
    let set = SnapshotIdSet::new();
    let set = set.set(64);
    assert!(set.get(64));
    assert!(!set.get(63));
    assert!(!set.get(128));

    let set = set.set(127);
    assert!(set.get(64));
    assert!(set.get(127));
    assert!(!set.get(128));
}

#[test]
fn test_set_idempotent() {
    let set = SnapshotIdSet::new();
    let set1 = set.set(10);
    let set2 = set1.set(10);
    assert_eq!(set1, set2);
}

#[test]
fn test_clear() {
    let set = SnapshotIdSet::new().set(10).set(20).set(30);
    assert!(set.get(10));
    assert!(set.get(20));
    assert!(set.get(30));

    let set = set.clear(20);
    assert!(set.get(10));
    assert!(!set.get(20));
    assert!(set.get(30));
}

#[test]
fn test_clear_idempotent() {
    let set = SnapshotIdSet::new().set(10);
    let set1 = set.clear(10);
    let set2 = set1.clear(10);
    assert_eq!(set1, set2);
}

#[test]
fn test_below_bound_insertion() {
    let mut set = SnapshotIdSet::new();
    set = set.set(100);
    assert_eq!(set.lower_bound, 0);

    set = set.set(50);
    assert!(set.get(50));
    assert!(set.get(100));

    set = set.set(25);
    set = set.set(75);
    assert!(set.get(25));
    assert!(set.get(50));
    assert!(set.get(75));
    assert!(set.get(100));

    let list = set.to_list();
    assert_eq!(list, vec![25, 50, 75, 100]);
}

#[test]
fn test_below_bound_removal() {
    let set = SnapshotIdSet::new();
    let set = set.set(25);
    let set = set.set(50);
    let set = set.set(75);
    let set = set.set(200);

    let set = set.clear(50);
    assert!(set.get(25));
    assert!(!set.get(50));
    assert!(set.get(75));
    assert!(set.get(200));

    let list = set.to_list();
    assert_eq!(list, vec![25, 75, 200]);
}

#[test]
fn test_shift_and_set() {
    let set = SnapshotIdSet::new();
    let set = set.set(10);
    assert_eq!(set.lower_bound, 0);

    let set = set.set(200);
    assert!(set.get(10));
    assert!(set.get(200));

    assert!(set.below_bound.is_some());
}

#[test]
fn test_shift_and_set_boundary_values() {
    let mut set = SnapshotIdSet::new();
    let boundary = SNAPSHOT_ID_SIZE * 12 - 1;
    set = set.set(boundary);
    assert!(set.get(boundary));

    set = set.set(boundary + 1);
    assert!(set.get(boundary));
    assert!(set.get(boundary + 1));
}

#[test]
fn test_set_below_lower_bound_inserts() {
    let set = SnapshotIdSet::new().set(200);
    let lower_bound = set.lower_bound;
    assert!(lower_bound > 0);

    let below = lower_bound - 1;
    let set = set.set(below);
    assert!(set.get(below));
    assert!(set.get(200));
}

#[test]
fn test_and_not_fast_path() {
    let set1 = SnapshotIdSet::new().set(10).set(20).set(30);
    let set2 = SnapshotIdSet::new().set(20).set(40);

    let result = set1.and_not(&set2);
    assert!(result.get(10));
    assert!(!result.get(20));
    assert!(result.get(30));
    assert!(!result.get(40));
}

#[test]
fn test_and_not_slow_path() {
    let set1 = SnapshotIdSet::new().set(10).set(20).set(30);
    let set2 = SnapshotIdSet::new().set(100).set(20);

    let result = set1.and_not(&set2);
    assert!(result.get(10));
    assert!(!result.get(20));
    assert!(result.get(30));
}

#[test]
fn test_or_fast_path() {
    let set1 = SnapshotIdSet::new().set(10).set(20);
    let set2 = SnapshotIdSet::new().set(20).set(30);

    let result = set1.or(&set2);
    assert!(result.get(10));
    assert!(result.get(20));
    assert!(result.get(30));
}

#[test]
fn test_or_slow_path() {
    let set1 = SnapshotIdSet::new().set(10).set(20);
    let set2 = SnapshotIdSet::new().set(100).set(30);

    let result = set1.or(&set2);
    assert!(result.get(10));
    assert!(result.get(20));
    assert!(result.get(30));
    assert!(result.get(100));
}

#[test]
fn test_lowest_in_below_bound() {
    let set = SnapshotIdSet::new();
    let set = set.set(25);
    let set = set.set(50);
    let set = set.set(200);
    assert_eq!(set.lowest(1000), 25);
    assert_eq!(set.lowest(100), 25);
    assert_eq!(set.lowest(30), 25);
}

#[test]
fn test_lowest_in_lower_set() {
    let set = SnapshotIdSet::new().set(10).set(20).set(30);
    assert_eq!(set.lowest(1000), 10);
    assert_eq!(set.lowest(25), 10);
}

#[test]
fn test_lowest_in_upper_set() {
    let set = SnapshotIdSet::new().set(70).set(80).set(90);
    assert_eq!(set.lowest(1000), 70);
}

#[test]
fn test_lowest_returns_upper_if_none_found() {
    let set = SnapshotIdSet::new().set(100);
    assert_eq!(set.lowest(50), 50);
}

#[test]
fn test_iterator() {
    let set = SnapshotIdSet::new().set(10).set(20).set(5).set(30);
    let list: Vec<_> = set.iter().collect();
    assert_eq!(list, vec![5, 10, 20, 30]);
}

#[test]
fn test_iterator_empty() {
    let set = SnapshotIdSet::new();
    let list: Vec<_> = set.iter().collect();
    assert_eq!(list, Vec::<SnapshotId>::new());
}

#[test]
fn test_iterator_all_ranges() {
    let set = SnapshotIdSet::new().set(5).set(10).set(70).set(200);

    let list: Vec<_> = set.iter().collect();
    assert_eq!(list, vec![5, 10, 70, 200]);
}

#[test]
fn test_to_list() {
    let set = SnapshotIdSet::new().set(10).set(20).set(30);
    assert_eq!(set.to_list(), vec![10, 20, 30]);
}

#[test]
fn test_debug_format() {
    let set = SnapshotIdSet::new().set(10).set(20);
    let debug_str = format!("{set:?}");
    assert_eq!(debug_str, "SnapshotIdSet{10, 20}");
}

#[test]
fn test_large_snapshot_ids() {
    let set = SnapshotIdSet::new();
    let set = set.set(500);
    let set = set.set(1000);
    let set = set.set(2000);

    assert!(set.get(500));
    assert!(set.get(1000));
    assert!(set.get(2000));
    assert!(!set.get(1500));
}

#[test]
fn test_boundary_transitions() {
    let set = SnapshotIdSet::new();

    let set = set.set(63);
    let set = set.set(64);
    assert!(set.get(63));
    assert!(set.get(64));

    let set = set.set(127);
    let set = set.set(128);
    assert!(set.get(127));
    assert!(set.get(128));
}
