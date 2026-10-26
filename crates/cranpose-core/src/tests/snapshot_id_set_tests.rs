use super::*;

#[test]
fn test_empty_set() {
    let set = SnapshotIdSet::EMPTY;
    assert!(set.is_empty());
    assert!(!set.get(SnapshotId(0)));
    assert!(!set.get(SnapshotId(100)));
}

#[test]
fn test_set_and_get_lower_range() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(0));
    assert!(set.get(SnapshotId(0)));
    assert!(!set.get(SnapshotId(1)));

    let set = set.set(SnapshotId(63));
    assert!(set.get(SnapshotId(0)));
    assert!(set.get(SnapshotId(63)));
    assert!(!set.get(SnapshotId(64)));
}

#[test]
fn test_set_and_get_upper_range() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(64));
    assert!(set.get(SnapshotId(64)));
    assert!(!set.get(SnapshotId(63)));
    assert!(!set.get(SnapshotId(128)));

    let set = set.set(SnapshotId(127));
    assert!(set.get(SnapshotId(64)));
    assert!(set.get(SnapshotId(127)));
    assert!(!set.get(SnapshotId(128)));
}

#[test]
fn test_set_idempotent() {
    let set = SnapshotIdSet::new();
    let set1 = set.set(SnapshotId(10));
    let set2 = set1.set(SnapshotId(10));
    assert_eq!(set1, set2);
}

#[test]
fn test_clear() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(30));
    assert!(set.get(SnapshotId(10)));
    assert!(set.get(SnapshotId(20)));
    assert!(set.get(SnapshotId(30)));

    let set = set.clear(SnapshotId(20));
    assert!(set.get(SnapshotId(10)));
    assert!(!set.get(SnapshotId(20)));
    assert!(set.get(SnapshotId(30)));
}

#[test]
fn test_clear_idempotent() {
    let set = SnapshotIdSet::new().set(SnapshotId(10));
    let set1 = set.clear(SnapshotId(10));
    let set2 = set1.clear(SnapshotId(10));
    assert_eq!(set1, set2);
}

#[test]
fn test_below_bound_insertion() {
    let mut set = SnapshotIdSet::new();
    set = set.set(SnapshotId(100));
    assert_eq!(set.lower_bound, 0);

    set = set.set(SnapshotId(50));
    assert!(set.get(SnapshotId(50)));
    assert!(set.get(SnapshotId(100)));

    set = set.set(SnapshotId(25));
    set = set.set(SnapshotId(75));
    assert!(set.get(SnapshotId(25)));
    assert!(set.get(SnapshotId(50)));
    assert!(set.get(SnapshotId(75)));
    assert!(set.get(SnapshotId(100)));

    let list = set.to_list();
    assert_eq!(
        list,
        vec![
            SnapshotId(25),
            SnapshotId(50),
            SnapshotId(75),
            SnapshotId(100)
        ]
    );
}

#[test]
fn test_below_bound_removal() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(25));
    let set = set.set(SnapshotId(50));
    let set = set.set(SnapshotId(75));
    let set = set.set(SnapshotId(200));

    let set = set.clear(SnapshotId(50));
    assert!(set.get(SnapshotId(25)));
    assert!(!set.get(SnapshotId(50)));
    assert!(set.get(SnapshotId(75)));
    assert!(set.get(SnapshotId(200)));

    let list = set.to_list();
    assert_eq!(list, vec![SnapshotId(25), SnapshotId(75), SnapshotId(200)]);
}

#[test]
fn test_shift_and_set() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(10));
    assert_eq!(set.lower_bound, 0);

    let set = set.set(SnapshotId(200));
    assert!(set.get(SnapshotId(10)));
    assert!(set.get(SnapshotId(200)));

    assert!(set.below_bound.is_some());
}

#[test]
fn test_shift_and_set_boundary_values() {
    let mut set = SnapshotIdSet::new();
    let boundary = SNAPSHOT_ID_SIZE * 12 - 1;
    set = set.set(SnapshotId(boundary));
    assert!(set.get(SnapshotId(boundary)));

    set = set.set(SnapshotId(boundary + 1));
    assert!(set.get(SnapshotId(boundary)));
    assert!(set.get(SnapshotId(boundary + 1)));
}

#[test]
fn test_set_below_lower_bound_inserts() {
    let set = SnapshotIdSet::new().set(SnapshotId(200));
    let lower_bound = set.lower_bound;
    assert!(lower_bound > 0);

    let below = lower_bound - 1;
    let set = set.set(SnapshotId(below));
    assert!(set.get(SnapshotId(below)));
    assert!(set.get(SnapshotId(200)));
}

#[test]
fn test_and_not_fast_path() {
    let set1 = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(30));
    let set2 = SnapshotIdSet::new().set(SnapshotId(20)).set(SnapshotId(40));

    let result = set1.and_not(&set2);
    assert!(result.get(SnapshotId(10)));
    assert!(!result.get(SnapshotId(20)));
    assert!(result.get(SnapshotId(30)));
    assert!(!result.get(SnapshotId(40)));
}

#[test]
fn test_and_not_slow_path() {
    let set1 = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(30));
    let set2 = SnapshotIdSet::new()
        .set(SnapshotId(100))
        .set(SnapshotId(20));

    let result = set1.and_not(&set2);
    assert!(result.get(SnapshotId(10)));
    assert!(!result.get(SnapshotId(20)));
    assert!(result.get(SnapshotId(30)));
}

#[test]
fn test_or_fast_path() {
    let set1 = SnapshotIdSet::new().set(SnapshotId(10)).set(SnapshotId(20));
    let set2 = SnapshotIdSet::new().set(SnapshotId(20)).set(SnapshotId(30));

    let result = set1.or(&set2);
    assert!(result.get(SnapshotId(10)));
    assert!(result.get(SnapshotId(20)));
    assert!(result.get(SnapshotId(30)));
}

#[test]
fn test_or_slow_path() {
    let set1 = SnapshotIdSet::new().set(SnapshotId(10)).set(SnapshotId(20));
    let set2 = SnapshotIdSet::new()
        .set(SnapshotId(100))
        .set(SnapshotId(30));

    let result = set1.or(&set2);
    assert!(result.get(SnapshotId(10)));
    assert!(result.get(SnapshotId(20)));
    assert!(result.get(SnapshotId(30)));
    assert!(result.get(SnapshotId(100)));
}

#[test]
fn test_lowest_in_below_bound() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(25));
    let set = set.set(SnapshotId(50));
    let set = set.set(SnapshotId(200));
    assert_eq!(set.lowest(SnapshotId(1000)), SnapshotId(25));
    assert_eq!(set.lowest(SnapshotId(100)), SnapshotId(25));
    assert_eq!(set.lowest(SnapshotId(30)), SnapshotId(25));
}

#[test]
fn test_lowest_in_lower_set() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(30));
    assert_eq!(set.lowest(SnapshotId(1000)), SnapshotId(10));
    assert_eq!(set.lowest(SnapshotId(25)), SnapshotId(10));
}

#[test]
fn test_lowest_in_upper_set() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(70))
        .set(SnapshotId(80))
        .set(SnapshotId(90));
    assert_eq!(set.lowest(SnapshotId(1000)), SnapshotId(70));
}

#[test]
fn test_lowest_returns_upper_if_none_found() {
    let set = SnapshotIdSet::new().set(SnapshotId(100));
    assert_eq!(set.lowest(SnapshotId(50)), SnapshotId(50));
}

#[test]
fn test_iterator() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(5))
        .set(SnapshotId(30));
    let list: Vec<_> = set.iter().collect();
    assert_eq!(
        list,
        vec![
            SnapshotId(5),
            SnapshotId(10),
            SnapshotId(20),
            SnapshotId(30)
        ]
    );
}

#[test]
fn test_iterator_empty() {
    let set = SnapshotIdSet::new();
    let list: Vec<_> = set.iter().collect();
    assert_eq!(list, Vec::<SnapshotId>::new());
}

#[test]
fn test_iterator_all_ranges() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(5))
        .set(SnapshotId(10))
        .set(SnapshotId(70))
        .set(SnapshotId(200));

    let list: Vec<_> = set.iter().collect();
    assert_eq!(
        list,
        vec![
            SnapshotId(5),
            SnapshotId(10),
            SnapshotId(70),
            SnapshotId(200)
        ]
    );
}

#[test]
fn test_to_list() {
    let set = SnapshotIdSet::new()
        .set(SnapshotId(10))
        .set(SnapshotId(20))
        .set(SnapshotId(30));
    assert_eq!(
        set.to_list(),
        vec![SnapshotId(10), SnapshotId(20), SnapshotId(30)]
    );
}

#[test]
fn test_debug_format() {
    let set = SnapshotIdSet::new().set(SnapshotId(10)).set(SnapshotId(20));
    let debug_str = format!("{set:?}");
    assert_eq!(debug_str, "SnapshotIdSet{10, 20}");
}

#[test]
fn test_large_snapshot_ids() {
    let set = SnapshotIdSet::new();
    let set = set.set(SnapshotId(500));
    let set = set.set(SnapshotId(1000));
    let set = set.set(SnapshotId(2000));

    assert!(set.get(SnapshotId(500)));
    assert!(set.get(SnapshotId(1000)));
    assert!(set.get(SnapshotId(2000)));
    assert!(!set.get(SnapshotId(1500)));
}

#[test]
fn test_boundary_transitions() {
    let set = SnapshotIdSet::new();

    let set = set.set(SnapshotId(63));
    let set = set.set(SnapshotId(64));
    assert!(set.get(SnapshotId(63)));
    assert!(set.get(SnapshotId(64)));

    let set = set.set(SnapshotId(127));
    let set = set.set(SnapshotId(128));
    assert!(set.get(SnapshotId(127)));
    assert!(set.get(SnapshotId(128)));
}
