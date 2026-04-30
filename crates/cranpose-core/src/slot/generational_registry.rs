use super::dense_id_map::DenseIdMap;
use crate::collections::map::HashMap;
use std::mem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GenerationalSlot<S> {
    pub(crate) generation: u32,
    pub(crate) state: S,
}

pub(crate) trait RegistryState: Copy {
    fn is_active(self) -> bool;
    fn is_detached(self) -> bool;
}

pub(crate) struct GenerationalRegistryStorage<S> {
    dense_states: DenseIdMap<GenerationalSlot<S>>,
    sparse_states: HashMap<usize, GenerationalSlot<S>>,
    active_count: usize,
    detached_count: usize,
}

impl<S> Default for GenerationalRegistryStorage<S> {
    fn default() -> Self {
        Self {
            dense_states: DenseIdMap::new(),
            sparse_states: HashMap::default(),
            active_count: 0,
            detached_count: 0,
        }
    }
}

impl<S> GenerationalRegistryStorage<S> {
    const DENSE_STORAGE_ID_LIMIT: usize = 65_536;

    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn slot(&self, id: usize) -> Option<&GenerationalSlot<S>> {
        if Self::uses_dense_storage(id) {
            self.dense_states.get(id)
        } else {
            self.sparse_states.get(&id)
        }
    }

    pub(crate) fn slot_mut(&mut self, id: usize) -> Option<&mut GenerationalSlot<S>> {
        if Self::uses_dense_storage(id) {
            self.dense_states.get_mut(id)
        } else {
            self.sparse_states.get_mut(&id)
        }
    }

    pub(crate) fn slots(&self) -> impl Iterator<Item = (usize, &GenerationalSlot<S>)> + '_ {
        self.dense_states
            .iter()
            .chain(self.sparse_states.iter().map(|(&id, slot)| (id, slot)))
    }

    pub(crate) fn slot_len(&self) -> usize {
        self.dense_states.len() + self.sparse_states.len()
    }

    pub(crate) fn sparse_slot_len(&self) -> usize {
        self.sparse_states.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.dense_states.capacity() + self.sparse_states.capacity()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.dense_states.capacity() * mem::size_of::<Option<GenerationalSlot<S>>>()
            + self.sparse_states.capacity() * mem::size_of::<(usize, GenerationalSlot<S>)>()
    }

    pub(crate) fn clear(&mut self) {
        self.dense_states.clear();
        self.sparse_states.clear();
        self.active_count = 0;
        self.detached_count = 0;
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.dense_states.shrink_to_fit();
        self.sparse_states.shrink_to_fit();
    }

    pub(crate) fn maybe_shrink_sparse_storage(&mut self) -> bool {
        if self.capacity() <= Self::DENSE_STORAGE_ID_LIMIT {
            return false;
        }
        if self.slot_len().saturating_mul(4) >= self.capacity() {
            return false;
        }
        self.shrink_to_fit();
        true
    }

    #[cfg(test)]
    pub(crate) fn dense_capacity(&self) -> usize {
        self.dense_states.capacity()
    }

    fn insert_slot(&mut self, id: usize, generation: u32, state: S) -> Option<GenerationalSlot<S>> {
        let slot = GenerationalSlot { generation, state };
        if Self::uses_dense_storage(id) {
            self.dense_states.insert(id, slot)
        } else {
            self.sparse_states.insert(id, slot)
        }
    }

    fn remove_slot(&mut self, id: usize) -> Option<GenerationalSlot<S>> {
        if Self::uses_dense_storage(id) {
            self.dense_states.remove(id)
        } else {
            self.sparse_states.remove(&id)
        }
    }

    fn uses_dense_storage(id: usize) -> bool {
        id <= Self::DENSE_STORAGE_ID_LIMIT
    }
}

impl<S: RegistryState> GenerationalRegistryStorage<S> {
    pub(crate) fn active_len(&self) -> usize {
        self.active_count
    }

    pub(crate) fn detached_len(&self) -> usize {
        self.detached_count
    }

    pub(crate) fn invalidated_len(&self) -> usize {
        self.slot_len()
            .saturating_sub(self.active_count + self.detached_count)
    }

    pub(crate) fn set_state(&mut self, id: usize, generation: u32, state: S) -> Option<S> {
        if let Some(slot) = self.slot(id) {
            if slot.generation != generation {
                return None;
            }
        }
        let previous = self.insert_slot(id, generation, state);
        self.adjust_state_counts(previous.map(|slot| slot.state), Some(state));
        previous.map(|slot| slot.state)
    }

    pub(crate) fn remove_state(&mut self, id: usize) -> Option<GenerationalSlot<S>> {
        let removed = self.remove_slot(id);
        if let Some(slot) = removed {
            self.adjust_state_counts(Some(slot.state), None);
        }
        removed
    }

    fn adjust_state_counts(&mut self, previous: Option<S>, next: Option<S>) {
        if previous.is_some_and(|state| state.is_active()) {
            self.active_count -= 1;
        }
        if previous.is_some_and(|state| state.is_detached()) {
            self.detached_count -= 1;
        }
        if next.is_some_and(|state| state.is_active()) {
            self.active_count += 1;
        }
        if next.is_some_and(|state| state.is_detached()) {
            self.detached_count += 1;
        }
    }
}
