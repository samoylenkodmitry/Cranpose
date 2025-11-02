//! Integration tests for the Snapshot V2 system.
//!
//! These tests exercise end-to-end behaviour using the real
//! `SnapshotMutableState` implementation to ensure snapshot isolation,
//! conflict detection, and observer dispatch behave as expected.

use super::*;
use crate::snapshot_v2::runtime::TestRuntimeGuard;
use crate::state::{MutationPolicy, NeverEqual, SnapshotMutableState, StateRecord};
use std::sync::Arc;

fn reset_runtime() -> TestRuntimeGuard {
    crate::snapshot_pinning::reset_pinning_table();
    reset_runtime_for_tests()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn new_state(initial: i32) -> Arc<SnapshotMutableState<i32>> {
        SnapshotMutableState::new_in_arc(initial, Arc::new(NeverEqual))
    }

    fn new_state_with_policy(
        initial: i32,
        policy: Arc<dyn MutationPolicy<i32>>,
    ) -> Arc<SnapshotMutableState<i32>> {
        SnapshotMutableState::new_in_arc(initial, policy)
    }

    struct SummingPolicy;

    impl MutationPolicy<i32> for SummingPolicy {
        fn equivalent(&self, a: &i32, b: &i32) -> bool {
            a == b
        }

        fn merge(&self, previous: &i32, current: &i32, applied: &i32) -> Option<i32> {
            let delta_current = *current - *previous;
            let delta_applied = *applied - *previous;
            Some(*previous + delta_current + delta_applied)
        }
    }

    #[test]
    fn test_end_to_end_simple_snapshot_workflow() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state(100);

        let snapshot1 = global.take_nested_mutable_snapshot(None, None);
        snapshot1.enter(|| {
            state.set(200);
            assert_eq!(state.get(), 200);
        });

        assert!(snapshot1.has_pending_changes());

        assert!(snapshot1.apply().is_success());
        assert_eq!(state.get(), 200);
    }

    #[test]
    fn test_concurrent_snapshots_with_different_objects() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state1 = new_state(1);
        let state2 = new_state(2);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state1.set(100));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state2.set(200));

        assert!(snap1.apply().is_success());
        assert!(snap2.apply().is_success());
        assert_eq!(state1.get(), 100);
        assert_eq!(state2.get(), 200);
    }

    #[test]
    fn test_concurrent_snapshots_with_same_object_conflict() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state(0);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(10));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state.set(20));

        assert!(snap1.apply().is_success(), "snap1 should succeed");
        assert!(
            snap2.apply().is_failure(),
            "snap2 should fail due to conflict with snap1"
        );
    }

    #[test]
    fn test_conflict_detection_after_record_reuse() {
        let _guard = reset_runtime();
        crate::snapshot_pinning::reset_pinning_table();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state(0);

        const INVALID_SNAPSHOT_ID: SnapshotId = 0;

        // Inject an INVALID record to force the next writable() call to reuse it.
        let head = state.first_record();
        let invalid_record = StateRecord::new(INVALID_SNAPSHOT_ID, -1i32, head.next());
        head.set_next(Some(invalid_record.clone()));

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        let snap2 = global.take_nested_mutable_snapshot(None, None);
        let snap1_id = snap1.snapshot_id();

        snap1.enter(|| state.set(10));

        // Reused record should now belong to snap1 and match the readable record for that snapshot.
        assert_eq!(
            invalid_record.snapshot_id(),
            snap1_id,
            "Writable reuse should update the recycled record's snapshot id"
        );
        let snap1_invalid = snap1.invalid();
        let readable = state.readable_record(snap1_id, &snap1_invalid);
        assert!(
            Arc::ptr_eq(&readable, &invalid_record),
            "Writable reuse should provide the recycled record as the readable head for the snapshot"
        );

        snap2.enter(|| state.set(20));

        assert!(
            snap1.apply().is_success(),
            "First snapshot should still apply successfully"
        );
        assert!(
            snap2.apply().is_failure(),
            "Second snapshot should detect the conflict after reuse"
        );
        assert_eq!(
            state.get(),
            10,
            "Global state should reflect the winning snapshot after conflict"
        );
    }

    #[test]
    fn test_optimistic_merges_success() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(SummingPolicy));

        let snapshot = global.take_nested_mutable_snapshot(None, None);
        snapshot.enter(|| state.set(10));

        // Competing snapshot advances global state to create a merge scenario.
        let competitor = global.take_nested_mutable_snapshot(None, None);
        competitor.enter(|| state.set(5));
        assert!(competitor.apply().is_success());

        let invalid = super::runtime::open_snapshots().clear(global.snapshot_id());
        let modified = snapshot.debug_modified_objects();
        let optimistic = super::optimistic_merges(
            global.snapshot_id(),
            snapshot.debug_base_parent_id(),
            &modified,
            &invalid,
        )
        .expect("expected optimistic merges");

        let head = state.first_record();
        let current =
            crate::state::readable_record_for(&head, global.snapshot_id(), &invalid).unwrap();
        let key = Arc::as_ptr(&current) as usize;

        let merged = optimistic.get(&key).expect("merged value present");
        let merged_value = merged.with_value(|value: &i32| *value);
        assert_eq!(
            merged_value, 15,
            "Merged value should sum applied and current deltas"
        );
    }

    #[test]
    fn test_optimistic_merges_failure_returns_none() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state(0);

        let snapshot = global.take_nested_mutable_snapshot(None, None);
        snapshot.enter(|| state.set(10));

        let competitor = global.take_nested_mutable_snapshot(None, None);
        competitor.enter(|| state.set(5));
        assert!(competitor.apply().is_success());

        let invalid = super::runtime::open_snapshots().clear(global.snapshot_id());
        let modified = snapshot.debug_modified_objects();
        let optimistic = super::optimistic_merges(
            global.snapshot_id(),
            snapshot.debug_base_parent_id(),
            &modified,
            &invalid,
        );

        assert!(
            optimistic.is_none(),
            "Non-mergeable conflicts should disable optimistic merges"
        );
    }

    struct LockDetectPolicy;

    impl MutationPolicy<i32> for LockDetectPolicy {
        fn equivalent(&self, a: &i32, b: &i32) -> bool {
            a == b
        }

        fn merge(&self, previous: &i32, current: &i32, applied: &i32) -> Option<i32> {
            assert_eq!(
                super::runtime::runtime_lock_depth(),
                0,
                "optimistic merges should execute outside the runtime lock"
            );
            let delta_current = *current - *previous;
            let delta_applied = *applied - *previous;
            Some(*previous + delta_current + delta_applied)
        }
    }

    #[test]
    fn test_optimistic_merges_runs_outside_runtime_lock() {
        let _guard = reset_runtime();
        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(LockDetectPolicy));

        let snapshot = global.take_nested_mutable_snapshot(None, None);
        snapshot.enter(|| state.set(10));

        let competitor = global.take_nested_mutable_snapshot(None, None);
        competitor.enter(|| state.set(5));
        assert!(competitor.apply().is_success());

        let invalid = super::runtime::open_snapshots().clear(global.snapshot_id());
        let modified = snapshot.debug_modified_objects();
        let optimistic = super::optimistic_merges(
            global.snapshot_id(),
            snapshot.debug_base_parent_id(),
            &modified,
            &invalid,
        )
        .expect("expected optimistic merge entries");

        assert!(
            !optimistic.is_empty(),
            "Mergeable conflict should yield optimistic results"
        );
    }

    #[test]
    fn test_nested_snapshot_applies_to_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let parent = global.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        let child = parent.take_nested_mutable_snapshot(None, None);

        child.enter(|| state.set(300));

        assert!(!parent.has_pending_changes());
        child.apply().check();
        assert!(parent.has_pending_changes());
        parent.apply().check();
        assert_eq!(state.get(), 300);
    }

    #[test]
    fn test_nested_snapshot_conflict_with_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let parent = global.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        parent.enter(|| state.set(100));
        let child = parent.take_nested_mutable_snapshot(None, None);
        child.enter(|| state.set(200));

        assert!(child.apply().is_failure());
        assert!(parent.has_pending_changes());
        parent.apply().check();
        assert_eq!(state.get(), 100);
    }

    #[test]
    fn test_observer_notifications_on_apply() {
        let _guard = reset_runtime();

        let called = Arc::new(Mutex::new(false));
        let received_count = Arc::new(Mutex::new(0));
        let called_clone = called.clone();
        let count_clone = received_count.clone();

        let _handle = register_apply_observer(Arc::new(move |modified, _snapshot_id| {
            *called_clone.lock().unwrap() = true;
            *count_clone.lock().unwrap() = modified.len();
        }));

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);
        let state1 = new_state(0);
        let state2 = new_state(0);

        snapshot.enter(|| {
            state1.set(10);
            state2.set(20);
        });

        snapshot.apply().check();
        assert!(*called.lock().unwrap());
        assert_eq!(*received_count.lock().unwrap(), 2);
    }

    #[test]
    fn test_three_way_merge_succeeds_with_policy() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(SummingPolicy));

        let warmup = global.take_nested_mutable_snapshot(None, None);
        warmup.enter(|| state.set(5));
        warmup.apply().check();
        assert_eq!(state.get(), 5);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(10));
        snap2.enter(|| state.set(20));

        snap1.apply().check();
        snap2.apply().check();
        assert_eq!(state.get(), 25);
    }

    #[test]
    fn test_three_way_merge_equivalent_prefers_parent() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state_with_policy(0, Arc::new(SummingPolicy));

        let warmup = global.take_nested_mutable_snapshot(None, None);
        warmup.enter(|| state.set(5));
        warmup.apply().check();
        assert_eq!(state.get(), 5);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        let snap2 = global.take_nested_mutable_snapshot(None, None);

        snap1.enter(|| state.set(50));
        snap2.enter(|| state.set(50));

        snap1.apply().check();
        snap2.apply().check();
        assert_eq!(state.get(), 50);
    }

    #[test]
    fn test_multiple_levels_of_nesting() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let level1 = global.take_nested_mutable_snapshot(None, None);
        let level2 = level1.take_nested_mutable_snapshot(None, None);
        let state = new_state(0);

        level2.enter(|| state.set(500));
        level2.apply().check();
        assert!(level1.has_pending_changes());

        level1.apply().check();
        assert_eq!(state.get(), 500);
    }

    #[test]
    fn test_snapshot_isolation() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let state = new_state(10);

        let snap1 = global.take_nested_mutable_snapshot(None, None);
        snap1.enter(|| state.set(20));

        let snap2 = global.take_nested_mutable_snapshot(None, None);
        snap2.enter(|| state.set(30));

        snap1.apply().check();
        assert!(
            snap2.apply().is_failure(),
            "snap2 should fail due to isolation rules"
        );
    }

    #[test]
    fn test_empty_snapshot_applies_successfully() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);

        assert!(snapshot.apply().is_success());
    }

    #[test]
    fn test_dispose_prevents_further_operations() {
        let _guard = reset_runtime();

        let global = GlobalSnapshot::get_or_create();
        let snapshot = global.take_nested_mutable_snapshot(None, None);

        snapshot.dispose();
        assert!(snapshot.apply().is_failure());
    }
}
