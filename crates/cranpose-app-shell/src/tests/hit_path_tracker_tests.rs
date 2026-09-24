use super::*;

#[test]
fn a_pointer_nobody_touched_has_no_dispatch_order() {
    let tracker = HitPathTracker::new();
    assert_eq!(tracker.dispatch_order(PointerId::PRIMARY), None);
}

#[test]
fn a_dispatch_order_puts_a_shared_ancestor_after_every_child_that_survived() {
    let mut tracker = HitPathTracker::new();
    tracker.add_hit_path(PointerId::PRIMARY, vec![vec![2, 1], vec![4, 1]]);

    let order = tracker
        .dispatch_order(PointerId::PRIMARY)
        .expect("a tracked pointer has an order");

    let ancestor = order
        .iter()
        .position(|node| *node == 1)
        .expect("the ancestor is dispatched");
    for child in [2, 4] {
        let at = order
            .iter()
            .position(|node| *node == child)
            .unwrap_or_else(|| panic!("child {child} was not dispatched"));
        assert!(
            at < ancestor,
            "child {child} was dispatched after its ancestor"
        );
    }
    assert_eq!(
        order.iter().filter(|node| **node == 1).count(),
        1,
        "the shared ancestor was dispatched twice"
    );
}

#[test]
fn test_add_and_get_path() {
    let mut tracker = HitPathTracker::new();
    let paths: Vec<Vec<NodeId>> = vec![vec![1, 2, 3], vec![4, 5]];

    tracker.add_hit_path(PointerId::PRIMARY, paths.clone());

    assert!(tracker.has_path(PointerId::PRIMARY));
    assert_eq!(tracker.get_path(PointerId::PRIMARY), Some(paths.as_slice()));
}

#[test]
fn test_remove_path() {
    let mut tracker = HitPathTracker::new();
    let paths: Vec<Vec<NodeId>> = vec![vec![1]];

    tracker.add_hit_path(PointerId::PRIMARY, paths.clone());
    let removed = tracker.remove_path(PointerId::PRIMARY);

    assert_eq!(removed, Some(paths));
    assert!(!tracker.has_path(PointerId::PRIMARY));
    assert!(tracker.is_empty());
}

#[test]
fn test_clear() {
    let mut tracker = HitPathTracker::new();
    tracker.add_hit_path(PointerId(0), vec![vec![1]]);
    tracker.add_hit_path(PointerId(1), vec![vec![2]]);

    assert!(!tracker.is_empty());

    tracker.clear();

    assert!(tracker.is_empty());
    assert!(!tracker.has_path(PointerId(0)));
    assert!(!tracker.has_path(PointerId(1)));
}

#[test]
fn test_multiple_pointers() {
    let mut tracker = HitPathTracker::new();
    let nodes1: Vec<Vec<NodeId>> = vec![vec![1]];
    let nodes2: Vec<Vec<NodeId>> = vec![vec![2, 3]];

    tracker.add_hit_path(PointerId(0), nodes1.clone());
    tracker.add_hit_path(PointerId(1), nodes2.clone());

    assert_eq!(tracker.get_path(PointerId(0)), Some(nodes1.as_slice()));
    assert_eq!(tracker.get_path(PointerId(1)), Some(nodes2.as_slice()));
}

#[test]
fn test_dispatch_order_keeps_shared_ancestor_after_overlapping_hits() {
    let paths = vec![vec![1, 99], vec![2, 99]];

    assert_eq!(dispatch_order_for_paths(&paths), vec![1, 2, 99]);
}

#[test]
fn dispatch_tree_build_does_not_panic_on_parent_entry_assumption() {
    let source = include_str!("../hit_path_tracker.rs");
    let parent_expect = ["expect(\"parent ", "inserted above\")"].concat();

    assert!(!source.contains(parent_expect.as_str()));
    assert!(source.contains("tree.entry(parent_id).or_default()"));
}
