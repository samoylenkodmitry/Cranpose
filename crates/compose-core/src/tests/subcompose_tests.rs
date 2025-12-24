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
    // RetainEvenPolicy keeps slot 2 (even), disposes slot 1 (odd)
    let disposed = state.dispose_or_reuse_starting_from_index(0);
    // Disposed returns empty because reusable pool not exceeded (max=7)
    assert!(disposed.is_empty());
    // But node 10 should be in reusable pool
    assert_eq!(state.reusable(), &[10]);
    assert_eq!(state.reusable_count, 1);
}

#[test]
fn dispose_from_middle_moves_trailing_slots() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[20], &[]);
    state.register_active(SlotId::new(3), &[30], &[]);
    // Dispose from index 2 - moves slot 3 to reusable
    let disposed = state.dispose_or_reuse_starting_from_index(2);
    // Disposed is empty because pool not exceeded
    assert!(disposed.is_empty());
    // Node 30 should be in reusable
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

    // Dispose returns empty (pool not exceeded), but nodes move to reusable
    let disposed = state.dispose_or_reuse_starting_from_index(0);
    assert!(disposed.is_empty());
    // All three nodes should be in reusable (order: 33, 22, 11 since LIFO from pop)
    assert_eq!(state.reusable().len(), 3);

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
    state.register_active(SlotId::new(1), &[10], std::slice::from_ref(&scope_a));
    state.register_active(SlotId::new(2), &[20], std::slice::from_ref(&scope_b));

    // Dispose from index 1 - moves slot 2 to reusable
    let disposed = state.dispose_or_reuse_starting_from_index(1);
    // Disposed is empty (pool not exceeded)
    assert!(disposed.is_empty());
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

#[test]
fn finish_pass_disposes_inactive_slots() {
    let mut state = SubcomposeState::default();
    state.begin_pass();
    state.register_active(SlotId::new(1), &[10], &[]);
    assert!(state.finish_pass().is_empty());

    // On second pass with no active slots, node 10 moves to reusable
    state.begin_pass();
    let disposed = state.finish_pass();
    // Disposed is empty (pool not exceeded)
    assert!(disposed.is_empty());
    // Node should be in reusable pool
    assert_eq!(state.reusable(), &[10]);
}

#[test]
fn finish_pass_keeps_active_slots() {
    let mut state = SubcomposeState::default();
    state.begin_pass();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[20], &[]);
    let disposed = state.finish_pass();
    assert!(disposed.is_empty());
    assert!(state.reusable().is_empty());
}

// ─── ContentTypeReusePolicy tests ────────────────────────────────────────────

#[test]
fn content_type_policy_allows_cross_slot_reuse() {
    let policy = ContentTypeReusePolicy::new();

    // Register same content type for different slots
    policy.set_content_type(SlotId::new(10), 1); // type 1
    policy.set_content_type(SlotId::new(20), 1); // type 1 (same)
    policy.set_content_type(SlotId::new(30), 2); // type 2 (different)

    // Same content type should be compatible
    assert!(policy.are_compatible(SlotId::new(10), SlotId::new(20)));
    assert!(policy.are_compatible(SlotId::new(20), SlotId::new(10)));

    // Different content type should not be compatible
    assert!(!policy.are_compatible(SlotId::new(10), SlotId::new(30)));
    assert!(!policy.are_compatible(SlotId::new(20), SlotId::new(30)));
}

#[test]
fn content_type_policy_exact_match_always_wins() {
    let policy = ContentTypeReusePolicy::new();

    // Exact match works even without registered content types
    assert!(policy.are_compatible(SlotId::new(5), SlotId::new(5)));

    // Exact match works with different content types (should still return true)
    policy.set_content_type(SlotId::new(5), 1);
    assert!(policy.are_compatible(SlotId::new(5), SlotId::new(5)));
}

#[test]
fn content_type_policy_unregistered_slots_not_compatible() {
    let policy = ContentTypeReusePolicy::new();

    // Only register one slot
    policy.set_content_type(SlotId::new(10), 1);

    // Unregistered slot should not be compatible (except exact match)
    assert!(!policy.are_compatible(SlotId::new(10), SlotId::new(99)));
    assert!(!policy.are_compatible(SlotId::new(99), SlotId::new(10)));
    assert!(!policy.are_compatible(SlotId::new(88), SlotId::new(99)));
}

#[test]
fn content_type_reuse_in_subcompose_state() {
    let policy = ContentTypeReusePolicy::new();

    // Set up content types: slots 1,3 are type A, slots 2 is type B
    policy.set_content_type(SlotId::new(1), 100);
    policy.set_content_type(SlotId::new(3), 100);
    policy.set_content_type(SlotId::new(2), 200);

    let mut state = SubcomposeState::new(Box::new(policy));

    // Activate slot 1 with node 10
    state.register_active(SlotId::new(1), &[10], &[]);

    // Move to reusable (simulating scroll-out)
    state.dispose_or_reuse_starting_from_index(0);
    assert_eq!(state.reusable(), &[10]);

    // Request slot 3 (same content type) - should reuse node 10
    let reused = state.take_node_from_reusables(SlotId::new(3));
    assert_eq!(
        reused,
        Some(10),
        "Should reuse node across slots with same content type"
    );
}
