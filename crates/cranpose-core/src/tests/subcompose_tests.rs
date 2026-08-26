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

struct SameTypeRejectsCrossSlotPolicy {
    types: std::cell::RefCell<HashMap<SlotId, u64>>,
}

impl SameTypeRejectsCrossSlotPolicy {
    fn new() -> Self {
        Self {
            types: std::cell::RefCell::new(HashMap::default()),
        }
    }
}

impl SlotReusePolicy for SameTypeRejectsCrossSlotPolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        HashSet::default()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        existing == requested
    }

    fn register_content_type(&self, slot_id: SlotId, content_type: u64) {
        self.types.borrow_mut().insert(slot_id, content_type);
    }

    fn remove_content_type(&self, slot_id: SlotId) {
        self.types.borrow_mut().remove(&slot_id);
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
fn exact_activation_rejects_inactive_slot_scopes() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.register_active(slot, &[10], std::slice::from_ref(&scope));
    state.dispose_or_reuse_starting_from_index(0);

    assert!(!scope.is_active());
    assert!(
        state.take_exact_slot_for_activation(slot).is_none(),
        "inactive retained slots must run subcomposition when they return"
    );
    assert_eq!(state.reusable(), vec![10]);
}

#[test]
fn exact_activation_of_active_slot_keeps_scopes_live() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.register_active(slot, &[10], std::slice::from_ref(&scope));

    let activation = state
        .take_exact_slot_activation(slot)
        .expect("active slots should exact-activate without subcomposition");

    assert_eq!(activation.nodes, vec![10]);
    assert_eq!(activation.scopes.len(), 1);
    assert!(
        !activation.reactivate_scopes,
        "active slots are already live and must not be treated as recycled"
    );
    assert!(scope.is_active());
    assert!(state.reusable().is_empty());
}

#[test]
fn sequential_active_slots_activate_through_cursor() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[20], &[]);
    state.register_active(SlotId::new(3), &[30], &[]);

    state.begin_pass();

    assert_eq!(
        state.activate_current_active_slot(SlotId::new(1)),
        Some(vec![10])
    );
    assert_eq!(state.active_slot_cursor(), 1);
    assert_eq!(
        state.activate_current_active_slot(SlotId::new(2)),
        Some(vec![20])
    );
    assert_eq!(state.active_slot_cursor(), 2);
    assert_eq!(
        state.activate_current_active_slot(SlotId::new(3)),
        Some(vec![30])
    );
    assert_eq!(state.active_slot_cursor(), 3);

    assert!(state.finish_pass().is_empty());
    assert!(state.reusable().is_empty());
}

#[test]
fn recycling_skipped_slot_restores_cursor_exact_activation() {
    let mut state = SubcomposeState::default();
    state.register_active(SlotId::new(0), &[10], &[]);
    state.register_active(SlotId::new(1), &[20], &[]);
    state.register_active(SlotId::new(2), &[30], &[]);

    state.begin_pass();

    assert_eq!(state.activate_current_active_slot(SlotId::new(1)), None);
    assert!(
        state
            .recycle_active_slots_where(|slot_id| slot_id.raw() < 1)
            .is_empty()
    );

    assert_eq!(
        state.activate_current_active_slot(SlotId::new(1)),
        Some(vec![20])
    );
    assert_eq!(state.active_slot_cursor(), 1);
}

#[test]
fn current_active_slot_activation_rejects_invalidated_scopes() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.register_active(slot, &[10], std::slice::from_ref(&scope));

    state.begin_pass();
    scope.invalidate();

    assert_eq!(state.activate_current_active_slot(slot), None);
    assert_eq!(state.active_slot_cursor(), 0);
}

#[test]
fn exact_activation_allows_prefetched_recycled_slots() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.register_active(slot, &[10], std::slice::from_ref(&scope));
    state.recycle_prefetched_active_slot(slot);

    assert!(!scope.is_active());
    let activation = state
        .take_exact_slot_activation(slot)
        .expect("prefetched slots should be exact-reactivated");
    assert_eq!(activation.nodes, vec![10]);
    assert_eq!(activation.scopes.len(), 1);
    assert!(
        activation.reactivate_scopes,
        "prefetched recycled slots must be reactivated when used"
    );
    assert!(state.reusable().is_empty());
}

#[test]
fn compatible_reuse_does_not_steal_prefetched_exact_slot() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));
    let exact_slot = SlotId::new(1);
    let requested_slot = SlotId::new(2);

    state.register_active(exact_slot, &[10], std::slice::from_ref(&scope));
    state.recycle_prefetched_active_slot(exact_slot);

    assert_eq!(
        state.take_node_from_reusables(requested_slot),
        None,
        "generic compatible reuse must not move a prefetched exact-reactivation slot"
    );

    let (nodes, scopes) = state
        .take_exact_slot_for_activation(exact_slot)
        .expect("prefetched exact slot must still be available");
    assert_eq!(nodes, vec![10]);
    assert_eq!(scopes.len(), 1);
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
fn draining_inactive_precomposed_uses_current_pass_activation() {
    let mut state = SubcomposeState::default();
    state.begin_pass();
    state.register_active(SlotId::new(1), &[10], &[]);
    assert!(state.finish_pass().is_empty());

    state.register_precomposed(SlotId::new(1), 99);

    state.begin_pass();
    let disposed = state.drain_inactive_precomposed();
    assert_eq!(disposed, vec![99]);
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

#[test]
fn restoring_active_slot_cursor_recycles_trailing_prefetch_slots() {
    let mut state = SubcomposeState::default();
    state.begin_pass();
    state.register_active(SlotId::new(1), &[10], &[]);
    let visible_cursor = state.active_slot_cursor();

    state.register_active(SlotId::new(2), &[20], &[]);
    state.register_active(SlotId::new(3), &[30], &[]);
    state.restore_active_slot_cursor(visible_cursor);

    let disposed = state.finish_pass();
    assert!(disposed.is_empty());
    assert_eq!(state.active_slots_count(), 1);
    assert_eq!(state.reusable(), &[30, 20]);
}

#[test]
fn recycling_active_slot_before_cursor_removes_it_from_rendered_set() {
    let mut state = SubcomposeState::default();
    state.begin_pass();
    state.register_active(SlotId::new(1), &[10], &[]);
    state.register_active(SlotId::new(2), &[20], &[]);
    state.register_active(SlotId::new(3), &[30], &[]);

    let disposed = state.recycle_active_slot(SlotId::new(2));
    assert!(disposed.is_empty());
    assert_eq!(state.active_slot_cursor(), 2);
    assert_eq!(state.active_slots_count(), 2);
    assert_eq!(state.reusable(), &[20]);

    assert!(state.finish_pass().is_empty());
    assert_eq!(state.reusable(), &[20]);
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

    policy.set_content_type(SlotId::new(10), 1);

    assert!(!policy.are_compatible(SlotId::new(10), SlotId::new(99)));
    assert!(!policy.are_compatible(SlotId::new(99), SlotId::new(10)));
    assert!(policy.are_compatible(SlotId::new(88), SlotId::new(99)));
}

#[test]
fn content_type_policy_reuses_untyped_slots_in_subcompose_state() {
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));
    state.register_active(SlotId::new(1), &[10], &[]);
    state.dispose_or_reuse_starting_from_index(0);

    assert_eq!(state.take_node_from_reusables(SlotId::new(2)), Some(10));
    assert_eq!(state.reusable_count, 0);
}

#[test]
fn cross_slot_reuse_moves_slot_host_and_callback_holder() {
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));
    let old_slot = SlotId::new(1);
    let new_slot = SlotId::new(2);
    let old_host = state.get_or_create_slots(old_slot);
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let old_holder = state.callback_holder(old_slot);
    let old_calls = std::rc::Rc::clone(&calls);
    old_holder.update(move || old_calls.borrow_mut().push("old"));
    let forwarded = old_holder.clone_rc();

    state.register_active(old_slot, &[10], &[]);
    state.dispose_or_reuse_starting_from_index(0);

    assert_eq!(state.take_node_from_reusables(new_slot), Some(10));
    let new_host = state.get_or_create_slots(new_slot);
    assert!(std::rc::Rc::ptr_eq(&old_host, &new_host));

    let new_holder = state.callback_holder(new_slot);
    let new_calls = std::rc::Rc::clone(&calls);
    new_holder.update(move || new_calls.borrow_mut().push("new"));
    forwarded();

    assert_eq!(calls.borrow().as_slice(), ["new"]);
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

#[test]
fn typed_reusable_pool_respects_policy_compatibility() {
    let mut state = SubcomposeState::new(Box::new(SameTypeRejectsCrossSlotPolicy::new()));

    state.register_content_type(SlotId::new(1), 100);
    state.register_content_type(SlotId::new(3), 100);
    state.register_active(SlotId::new(1), &[10], &[]);
    state.dispose_or_reuse_starting_from_index(0);

    let reused = state.take_node_from_reusables(SlotId::new(3));

    assert_eq!(
        reused, None,
        "same-type pool must not bypass SlotReusePolicy::are_compatible"
    );
    assert_eq!(
        state.reusable(),
        vec![10],
        "incompatible typed candidates must stay reusable for a later compatible slot"
    );
}

#[test]
fn content_type_none_clears_policy() {
    // This test verifies that transitioning a slot's content type to None
    // properly clears the policy mapping, preventing stale type-based reuse.

    // Test via SubcomposeState which uses ContentTypeReusePolicy
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));

    // Register slot 1 with content type 100
    state.register_content_type(SlotId::new(1), 100);
    state.register_active(SlotId::new(1), &[10], &[]);

    // Verify type is set
    assert_eq!(state.get_content_type(SlotId::new(1)), Some(100));

    // Transition to None - this should clear BOTH local and policy mappings
    state.update_content_type(SlotId::new(1), None);

    // Verify local mapping cleared
    assert_eq!(state.get_content_type(SlotId::new(1)), None);

    // Move slot 1 to reusable (goes to untyped pool since type was cleared)
    state.dispose_or_reuse_starting_from_index(0);

    // Register slot 2 with content type 100
    state.register_content_type(SlotId::new(2), 100);

    // KEY ASSERTION: Since slot 1's type was cleared, it's in the untyped pool.
    // ContentTypeReusePolicy::are_compatible returns false for (untyped, typed) pairs.
    // Therefore, the node should NOT be returned for a typed slot request.
    let reused = state.take_node_from_reusables(SlotId::new(2));
    assert_eq!(
        reused, None,
        "Untyped slot should not match typed slot 2 (type 100)"
    );
}

#[test]
fn moving_last_reusable_node_transfers_slot_composition() {
    let mut state = SubcomposeState::new(Box::new(ContentTypeReusePolicy::new()));
    let slot_a = SlotId::new(1);
    let slot_b = SlotId::new(2);

    state.register_content_type(slot_a, 7);
    state.register_content_type(slot_b, 7);
    let host = state.get_or_create_slots(slot_a);
    let _callback = state.callback_holder(slot_a);

    state.register_active(slot_a, &[10], &[]);
    assert!(state.dispose_or_reuse_starting_from_index(0).is_empty());
    assert!(state.slot_compositions.contains_key(&slot_a));
    assert!(state.slot_callbacks.contains_key(&slot_a));
    assert_eq!(state.get_content_type(slot_a), Some(7));
    assert_eq!(state.reusable_count, 1);

    let reused = state.take_node_from_reusables(slot_b);
    assert_eq!(reused, Some(10));
    let transferred_host = state.get_or_create_slots(slot_b);
    assert!(std::rc::Rc::ptr_eq(&host, &transferred_host));
    assert!(!state.slot_compositions.contains_key(&slot_a));
    assert!(!state.slot_callbacks.contains_key(&slot_a));
    assert_eq!(state.get_content_type(slot_a), None);
    assert_eq!(state.get_content_type(slot_b), Some(7));
    assert_eq!(state.reusable_count, 0);
    assert!(!state.reusable_node_counts.contains_key(&slot_a));
}

#[test]
fn finish_pass_disposes_overflow_node_and_preserves_slot_composition() {
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);

    state.max_reusable_per_type = 0;
    state.register_content_type(slot, 9);
    let _host = state.get_or_create_slots(slot);
    let _callback = state.callback_holder(slot);

    state.begin_pass();
    state.register_active(slot, &[10], &[]);
    assert!(state.finish_pass().is_empty());
    assert!(state.slot_compositions.contains_key(&slot));
    assert!(state.slot_callbacks.contains_key(&slot));

    state.begin_pass();
    let disposed = state.finish_pass();
    assert_eq!(disposed, vec![10]);
    assert!(state.slot_compositions.contains_key(&slot));
    assert!(state.slot_callbacks.contains_key(&slot));
    assert_eq!(state.get_content_type(slot), Some(9));
    assert_eq!(state.reusable_count, 0);
    assert!(!state.reusable_node_counts.contains_key(&slot));
}

#[test]
fn reusable_pool_removal_tolerates_missing_slot_count() {
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state.reusable_nodes_untyped.push_back((slot, 10));
    state.reusable_count = 1;

    let removed = state.remove_from_reusable_pools(10);

    assert_eq!(removed, Some(slot));
    assert!(state.reusable_nodes_untyped.is_empty());
    assert_eq!(state.reusable_count, 0);
}

#[test]
fn typed_reusable_pool_removal_tolerates_missing_slot_count() {
    let mut state = SubcomposeState::default();
    let slot = SlotId::new(1);
    state
        .reusable_by_type
        .entry(7)
        .or_default()
        .push_back((slot, 10));
    state.reusable_count = 1;

    let removed = state.remove_from_reusable_pools(10);

    assert_eq!(removed, Some(slot));
    assert!(state.reusable_by_type.is_empty());
    assert_eq!(state.reusable_count, 0);
}
