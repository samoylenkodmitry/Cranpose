use super::*;
use crate::tests::state_mock::MockState;

#[test]
fn test_weak_set_new() {
    let set = SnapshotWeakSet::new();
    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
}

#[test]
fn test_weak_set_add_single() {
    let mut set = SnapshotWeakSet::new();
    let state = MockState::tracked(42);

    set.add(&state);
    assert_eq!(set.len(), 1);
    assert_eq!(set.alive_count(), 1);
}

#[test]
fn test_weak_set_add_multiple() {
    let mut set = SnapshotWeakSet::new();
    let state1 = MockState::tracked(1);
    let state2 = MockState::tracked(2);
    let state3 = MockState::tracked(3);

    set.add(&state1);
    set.add(&state2);
    set.add(&state3);

    assert_eq!(set.len(), 3);
    assert_eq!(set.alive_count(), 3);
}

#[test]
fn test_weak_set_maintains_sort_order() {
    let mut set = SnapshotWeakSet::new();
    let states: Vec<_> = (0..10).map(MockState::tracked).collect();

    for state in &states {
        set.add(state);
    }

    let hashes: Vec<_> = set.entries.iter().map(|(h, _)| *h).collect();
    let mut sorted_hashes = hashes.clone();
    sorted_hashes.sort_unstable();
    assert_eq!(hashes, sorted_hashes, "Entries should be sorted by hash");
}

#[test]
fn test_weak_set_removes_dead_references() {
    let mut set = SnapshotWeakSet::new();

    {
        let state1 = MockState::tracked(1);
        let state2 = MockState::tracked(2);
        set.add(&state1);
        set.add(&state2);
        assert_eq!(set.alive_count(), 2);
    }

    assert_eq!(set.len(), 2);
    assert_eq!(set.alive_count(), 0);

    set.remove_if(|_| true);
    assert_eq!(set.len(), 0);
    assert!(set.is_empty());
}

#[test]
fn test_weak_set_remove_if_predicate() {
    let mut set = SnapshotWeakSet::new();
    let state1 = MockState::tracked(1);
    let state2 = MockState::tracked(2);
    let state3 = MockState::tracked(3);

    set.add(&state1);
    set.add(&state2);
    set.add(&state3);

    set.remove_if(|state: &dyn StateObject| {
        let mock = state.as_any().downcast_ref::<MockState>().unwrap();
        mock.value() % 2 != 0
    });

    assert_eq!(set.alive_count(), 2);
}

#[test]
fn test_weak_set_mixed_alive_and_dead() {
    let mut set = SnapshotWeakSet::new();
    let state1 = MockState::tracked(1);

    set.add(&state1);

    {
        let state2 = MockState::tracked(2);
        set.add(&state2);
    }

    let state3 = MockState::tracked(3);
    set.add(&state3);

    assert_eq!(set.len(), 3);
    assert_eq!(set.alive_count(), 2);

    set.remove_if(|_| true);
    assert_eq!(set.len(), 2);
    assert_eq!(set.alive_count(), 2);
}

#[test]
fn test_weak_set_capacity_growth() {
    let mut set = SnapshotWeakSet::new();
    let initial_capacity = set.entries.capacity();

    let states: Vec<_> = (0..20).map(MockState::tracked).collect();
    for state in &states {
        set.add(state);
    }

    assert!(
        set.entries.capacity() > initial_capacity,
        "Capacity should have grown"
    );
    assert_eq!(set.alive_count(), 20);
}

#[test]
fn test_weak_set_remove_if_keeps_matching() {
    let mut set = SnapshotWeakSet::new();
    let state1 = MockState::tracked(10);
    let state2 = MockState::tracked(20);
    let state3 = MockState::tracked(30);

    set.add(&state1);
    set.add(&state2);
    set.add(&state3);

    set.remove_if(|state: &dyn StateObject| {
        let mock = state.as_any().downcast_ref::<MockState>().unwrap();
        mock.value() >= 20
    });

    assert_eq!(set.alive_count(), 2);
}

#[test]
fn test_weak_set_remove_all() {
    let mut set = SnapshotWeakSet::new();
    let states: Vec<_> = (0..5).map(MockState::tracked).collect();

    for state in &states {
        set.add(state);
    }

    assert_eq!(set.alive_count(), 5);

    set.remove_if(|_| false);
    assert!(set.is_empty());
    assert_eq!(set.alive_count(), 0);
}
