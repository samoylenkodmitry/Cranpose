use std::sync::{Arc, Weak};

use crate::state::StateObject;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SnapshotWeakSetDebugStats {
    pub len: usize,
    pub capacity: usize,
}

pub(crate) struct SnapshotWeakSet {
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
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::with_capacity(16),
        }
    }

    #[cfg(test)]
    pub(crate) fn add<T: StateObject + 'static>(&mut self, state: &Arc<T>) {
        let hash = Arc::as_ptr(state) as *const () as usize;
        let trait_obj: Arc<dyn StateObject> = state.clone();
        let weak = Arc::downgrade(&trait_obj);

        let pos = self.entries.partition_point(|(h, _)| *h < hash);

        let has_live = self.entries[pos..]
            .iter()
            .take_while(|(h, _)| *h == hash)
            .any(|(_, existing)| existing.upgrade().is_some());
        if has_live {
            return;
        }

        self.entries.insert(pos, (hash, weak));

        if self.entries.len() == self.entries.capacity() {
            self.entries.reserve(self.entries.len());
        }
    }

    pub(crate) fn add_trait_object(&mut self, state: &Arc<dyn StateObject>) {
        let hash = Arc::as_ptr(state) as *const () as usize;
        let weak = Arc::downgrade(state);

        let pos = self.entries.partition_point(|(h, _)| *h < hash);

        let has_live = self.entries[pos..]
            .iter()
            .take_while(|(h, _)| *h == hash)
            .any(|(_, existing)| existing.upgrade().is_some());
        if has_live {
            return;
        }

        self.entries.insert(pos, (hash, weak));

        if self.entries.len() == self.entries.capacity() {
            self.entries.reserve(self.entries.len());
        }
    }

    pub(crate) fn remove_if<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&dyn StateObject) -> bool,
    {
        self.entries.retain(|(_, weak)| {
            if let Some(strong) = weak.upgrade() {
                predicate(&*strong)
            } else {
                false
            }
        });
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn debug_stats(&self) -> SnapshotWeakSetDebugStats {
        SnapshotWeakSetDebugStats {
            len: self.entries.len(),
            capacity: self.entries.capacity(),
        }
    }

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
#[allow(clippy::arc_with_non_send_sync)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::RwLock};

    use super::*;
    use crate::{
        snapshot_id_set::{SnapshotId, SnapshotIdSet},
        state::ObjectId,
    };

    struct MockState {
        id: ObjectId,
        value: Cell<i32>,
        head: RwLock<Rc<crate::state::StateRecord>>,
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

        fn first_record(&self) -> Rc<crate::state::StateRecord> {
            self.head.read().unwrap().clone()
        }

        fn try_readable_record(
            &self,
            snapshot_id: SnapshotId,
            invalid: &SnapshotIdSet,
        ) -> Option<Rc<crate::state::StateRecord>> {
            Some(self.readable_record(snapshot_id, invalid))
        }

        fn readable_record(
            &self,
            _snapshot_id: SnapshotId,
            _invalid: &SnapshotIdSet,
        ) -> Rc<crate::state::StateRecord> {
            self.head.read().unwrap().clone()
        }

        fn prepend_state_record(&self, record: Rc<crate::state::StateRecord>) {
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
        let states: Vec<_> = (0..10).map(MockState::new).collect();

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
            let state1 = MockState::new(1);
            let state2 = MockState::new(2);
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
        let state1 = MockState::new(1);
        let state2 = MockState::new(2);
        let state3 = MockState::new(3);

        set.add(&state1);
        set.add(&state2);
        set.add(&state3);

        set.remove_if(|state: &dyn StateObject| {
            let mock = state.as_any().downcast_ref::<MockState>().unwrap();
            mock.value.get() % 2 != 0
        });

        assert_eq!(set.alive_count(), 2);
    }

    #[test]
    fn test_weak_set_mixed_alive_and_dead() {
        let mut set = SnapshotWeakSet::new();
        let state1 = MockState::new(1);

        set.add(&state1);

        {
            let state2 = MockState::new(2);
            set.add(&state2);
        }

        let state3 = MockState::new(3);
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

        let states: Vec<_> = (0..20).map(MockState::new).collect();
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

        set.remove_if(|state: &dyn StateObject| {
            let mock = state.as_any().downcast_ref::<MockState>().unwrap();
            mock.value.get() >= 20
        });

        assert_eq!(set.alive_count(), 2);
    }

    #[test]
    fn test_weak_set_remove_all() {
        let mut set = SnapshotWeakSet::new();
        let states: Vec<_> = (0..5).map(MockState::new).collect();

        for state in &states {
            set.add(state);
        }

        assert_eq!(set.alive_count(), 5);

        set.remove_if(|_| false);
        assert!(set.is_empty());
        assert_eq!(set.alive_count(), 0);
    }
}
