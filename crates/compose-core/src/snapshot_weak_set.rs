//! Weak reference set for tracking state objects with multiple records.
//!
//! This implements Kotlin's `SnapshotWeakSet` - a collection that maintains weak
//! references to state objects, automatically removing dead references and providing
//! efficient add/remove operations via binary search.

use crate::state::StateObject;
use std::sync::{Arc, Weak};

/// A sorted set of weak references to StateObjects, optimized for memory cleanup.
///
/// The set maintains elements sorted by their identity hash (pointer address) to
/// enable O(log N) lookups and insertions via binary search. Weak references
/// prevent memory leaks - GC'd objects are automatically removed during iteration.
pub(crate) struct SnapshotWeakSet {
    /// Sorted array of (hash, weak_ref) pairs
    entries: Vec<(usize, Weak<dyn StateObject>)>,
}

impl std::fmt::Debug for SnapshotWeakSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotWeakSet")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

impl SnapshotWeakSet {
    /// Create a new empty weak set with default capacity.
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::with_capacity(16),
        }
    }

    /// Add a state object to the set.
    ///
    /// Uses binary search to find the insertion point, maintaining sort order.
    /// Duplicates are allowed (same hash can appear multiple times).
    #[allow(dead_code)]
    pub(crate) fn add<T: StateObject + 'static>(&mut self, state: &Arc<T>) {
        let hash = Arc::as_ptr(state) as *const () as usize;
        let trait_obj: Arc<dyn StateObject> = state.clone();
        let weak = Arc::downgrade(&trait_obj);

        // Binary search to find insertion point
        let pos = self.entries.partition_point(|(h, _)| *h < hash);

        self.entries.insert(pos, (hash, weak));

        // Grow capacity if needed (double when full)
        if self.entries.len() == self.entries.capacity() {
            self.entries.reserve(self.entries.len());
        }
    }

    /// Add a trait object to the set (for use with Arc<dyn StateObject>).
    ///
    /// This is a specialized version of `add` that works with trait objects directly.
    #[allow(dead_code)]
    pub(crate) fn add_trait_object(&mut self, state: &Arc<dyn StateObject>) {
        let hash = Arc::as_ptr(state) as *const () as usize;
        let weak = Arc::downgrade(state);

        // Binary search to find insertion point
        let pos = self.entries.partition_point(|(h, _)| *h < hash);

        self.entries.insert(pos, (hash, weak));

        // Grow capacity if needed (double when full)
        if self.entries.len() == self.entries.capacity() {
            self.entries.reserve(self.entries.len());
        }
    }

    /// Remove entries based on a predicate, also cleaning up dead weak references.
    ///
    /// The predicate receives a reference to the StateObject and should return:
    /// - `true` to keep the entry in the set
    /// - `false` to remove the entry
    ///
    /// Dead weak references (GC'd objects) are automatically removed regardless
    /// of the predicate.
    pub(crate) fn remove_if<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&dyn StateObject) -> bool,
    {
        self.entries.retain(|(_, weak)| {
            // Try to upgrade the weak reference
            if let Some(strong) = weak.upgrade() {
                // Object still alive - check predicate
                predicate(&*strong)
            } else {
                // Object was GC'd - remove it
                false
            }
        });
    }

    /// Get the current number of entries (including dead references).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the set is empty.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Count the number of alive entries (for testing).
    #[cfg(test)]
    pub(crate) fn alive_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, weak)| weak.upgrade().is_some())
            .count()
    }
}

impl Default for SnapshotWeakSet {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot_id_set::{SnapshotId, SnapshotIdSet};
    use crate::state::ObjectId;
    use std::cell::Cell;
    use std::sync::RwLock;

    // Mock StateObject for testing
    struct MockState {
        id: ObjectId,
        value: Cell<i32>,
        head: RwLock<Arc<crate::state::StateRecord>>,
    }

    impl MockState {
        fn new(value: i32) -> Arc<Self> {
            use crate::state::StateRecord;
            let record = StateRecord::new(1, value, None);
            let mut state = Arc::new(Self {
                id: ObjectId::default(),
                value: Cell::new(value),
                head: RwLock::new(record),
            });
            let id = ObjectId::new(&state);
            Arc::get_mut(&mut state).unwrap().id = id;
            state
        }
    }

    impl StateObject for MockState {
        fn object_id(&self) -> ObjectId {
            self.id
        }

        fn first_record(&self) -> Arc<crate::state::StateRecord> {
            self.head.read().unwrap().clone()
        }

        fn readable_record(
            &self,
            _snapshot_id: SnapshotId,
            _invalid: &SnapshotIdSet,
        ) -> Arc<crate::state::StateRecord> {
            self.head.read().unwrap().clone()
        }

        fn prepend_state_record(&self, record: Arc<crate::state::StateRecord>) {
            *self.head.write().unwrap() = record;
        }

        fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_weak_set_new() {
        let set = SnapshotWeakSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_weak_set_add_single() {
        let mut set = SnapshotWeakSet::new();
        let state = MockState::new(42);

        set.add(&state);
        assert_eq!(set.len(), 1);
        assert_eq!(set.alive_count(), 1);
    }

    #[test]
    fn test_weak_set_add_multiple() {
        let mut set = SnapshotWeakSet::new();
        let state1 = MockState::new(1);
        let state2 = MockState::new(2);
        let state3 = MockState::new(3);

        set.add(&state1);
        set.add(&state2);
        set.add(&state3);

        assert_eq!(set.len(), 3);
        assert_eq!(set.alive_count(), 3);
    }

    #[test]
    fn test_weak_set_maintains_sort_order() {
        let mut set = SnapshotWeakSet::new();
        let states: Vec<_> = (0..10).map(|i| MockState::new(i)).collect();

        // Add in random order
        for state in &states {
            set.add(state);
        }

        // Verify entries are sorted by hash
        let hashes: Vec<_> = set.entries.iter().map(|(h, _)| *h).collect();
        let mut sorted_hashes = hashes.clone();
        sorted_hashes.sort_unstable();
        assert_eq!(hashes, sorted_hashes, "Entries should be sorted by hash");
    }

    #[test]
    fn test_weak_set_removes_dead_references() {
        let mut set = SnapshotWeakSet::new();

        {
            let state1 = MockState::new(1);
            let state2 = MockState::new(2);
            set.add(&state1);
            set.add(&state2);
            assert_eq!(set.alive_count(), 2);
            // state1 and state2 drop here
        }

        // Dead references are still in the set until removeIf is called
        assert_eq!(set.len(), 2);
        assert_eq!(set.alive_count(), 0);

        // removeIf with always-true predicate should remove dead refs
        set.remove_if(|_| true);
        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
    }

    #[test]
    fn test_weak_set_remove_if_predicate() {
        let mut set = SnapshotWeakSet::new();
        let state1 = MockState::new(1);
        let state2 = MockState::new(2);
        let state3 = MockState::new(3);

        set.add(&state1);
        set.add(&state2);
        set.add(&state3);

        // Remove states with even values
        set.remove_if(|state: &dyn StateObject| {
            let mock = state.as_any().downcast_ref::<MockState>().unwrap();
            mock.value.get() % 2 != 0 // Keep odd values
        });

        assert_eq!(set.alive_count(), 2); // Should have 1 and 3
    }

    #[test]
    fn test_weak_set_mixed_alive_and_dead() {
        let mut set = SnapshotWeakSet::new();
        let state1 = MockState::new(1);

        set.add(&state1);

        {
            let state2 = MockState::new(2);
            set.add(&state2);
            // state2 drops here
        }

        let state3 = MockState::new(3);
        set.add(&state3);

        assert_eq!(set.len(), 3);
        assert_eq!(set.alive_count(), 2); // state1 and state3

        // Clean up dead references
        set.remove_if(|_| true);
        assert_eq!(set.len(), 2);
        assert_eq!(set.alive_count(), 2);
    }

    #[test]
    fn test_weak_set_capacity_growth() {
        let mut set = SnapshotWeakSet::new();
        let initial_capacity = set.entries.capacity();

        // Add more than initial capacity
        let states: Vec<_> = (0..20).map(|i| MockState::new(i)).collect();
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
        let state1 = MockState::new(10);
        let state2 = MockState::new(20);
        let state3 = MockState::new(30);

        set.add(&state1);
        set.add(&state2);
        set.add(&state3);

        // Keep only states with value >= 20
        set.remove_if(|state: &dyn StateObject| {
            let mock = state.as_any().downcast_ref::<MockState>().unwrap();
            mock.value.get() >= 20
        });

        assert_eq!(set.alive_count(), 2); // Should have state2 and state3
    }

    #[test]
    fn test_weak_set_remove_all() {
        let mut set = SnapshotWeakSet::new();
        let states: Vec<_> = (0..5).map(|i| MockState::new(i)).collect();

        for state in &states {
            set.add(state);
        }

        assert_eq!(set.alive_count(), 5);

        // Remove everything
        set.remove_if(|_| false);
        assert!(set.is_empty());
        assert_eq!(set.alive_count(), 0);
    }
}
