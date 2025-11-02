use super::*;
use crate::TestRuntime;

struct RetainEvenPolicy;

impl SlotReusePolicy for RetainEvenPolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        active
            .iter()
            .copied()
            .filter(|slot| slot.raw() % 2 == 0)
            .collect()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        existing == requested
    }
}

struct ParityPolicy;

impl SlotReusePolicy for ParityPolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        HashSet::default()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        existing.raw() % 2 == requested.raw() % 2
    }
}

#[test]
fn exact_reuse_wins() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(state.reusable(), &[10]);
    let reused = state.take_node_from_reusables(SlotId::new(1));
    assert_eq!(reused, Some(10));
}

#[test]
fn policy_based_compatibility() {
    let mut state = SubcomposeState::new(Box::new(ParityPolicy));
    state.register_active(SlotId::new(2), &[42], &[]);
    state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(state.reusable(), &[42]);
    let reused = state.take_node_from_reusables(SlotId::new(4));
    assert_eq!(reused, Some(42));
}

#[test]
fn dispose_or_reuse_respects_policy() {
    let mut state = SubcomposeState::new(Box::new(RetainEvenPolicy));
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[11], &[]);
    let moved = state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(moved, vec![10]);
    assert_eq!(state.reusable_count, 1);
}

#[test]
fn dispose_from_middle_moves_trailing_slots() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[20], &[]);
    state.register_active(SlotId::new(3), &[30], &[]);
    let moved = state.dispose_or_reuse_starting_from_index(2);
    assert_eq!(moved, vec![30]);
    assert_eq!(state.reusable(), &[30]);
    assert_eq!(state.reusable_count, 1);
    assert!(state.dispose_or_reuse_starting_from_index(5).is_empty());
}

#[test]
fn incompatible_reuse_is_rejected() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(state.take_node_from_reusables(SlotId::new(2)), None);
    assert_eq!(state.reusable(), &[10]);
}

#[test]
fn reordering_keyed_children_preserves_nodes() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[11], &[]);
    state.register_active(SlotId::new(2), &[22], &[]);
    state.register_active(SlotId::new(3), &[33], &[]);

    let moved = state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(moved, vec![33, 22, 11]);

    let reordered = [SlotId::new(3), SlotId::new(1), SlotId::new(2)];
    let mut reused_nodes = Vec::new();
    for slot in reordered {
        let node = state
            .take_node_from_reusables(slot)
            .expect("expected node for reordered slot");
        reused_nodes.push(node);
        state.register_active(slot, &[node], &[]);
    }

    assert_eq!(reused_nodes, vec![33, 11, 22]);
    assert!(state.reusable().is_empty());
    assert_eq!(state.reusable_count, 0);
}

#[test]
fn removing_slots_deactivates_scopes() {
    let runtime = TestRuntime::new();
    let scope_a = RecomposeScope::new_for_test(runtime.handle());
    let scope_b = RecomposeScope::new_for_test(runtime.handle());

    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[scope_a.clone()]);
    state.register_active(SlotId::new(2), &[20], &[scope_b.clone()]);

    let moved = state.dispose_or_reuse_starting_from_index(1);
    assert_eq!(moved, vec![20]);
    assert!(scope_a.is_active());
    assert!(!scope_b.is_active());
    assert_eq!(state.reusable(), &[20]);
}

#[test]
fn draining_inactive_precomposed_returns_nodes() {
    let mut state = SubcomposeState::default();
    state.register_precomposed(SlotId::new(7), 77);
    state.register_active(SlotId::new(8), &[88], &[]);
    let disposed = state.drain_inactive_precomposed();
    assert_eq!(disposed, vec![77]);
    assert!(state.precomposed().is_empty());
}
