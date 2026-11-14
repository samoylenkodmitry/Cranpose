use super::dense_id_map::DenseIdMap;
use crate::collections::growth_slack::GrowthSlack;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GenerationalSlot<S> {
    pub(crate) generation: u32,
    pub(crate) state: S,
}

pub(crate) trait RegistryState: Copy {
    fn is_active(self) -> bool;
    fn is_detached(self) -> bool;
}

// The states of a registry's ids, by id: ids are handed out lowest free
// first, so they stay as many as the most ids alive at once.
pub(crate) struct GenerationalRegistryStorage<S> {
    states: DenseIdMap<GenerationalSlot<S>>,
    active_count: usize,
    detached_count: usize,
}

impl<S> Default for GenerationalRegistryStorage<S> {
    fn default() -> Self {
        Self {
            states: DenseIdMap::new(),
            active_count: 0,
            detached_count: 0,
        }
    }
}

// The room below which an emptied registry keeps its storage.
const SHRINK_FLOOR: usize = 1_024;

impl<S> GenerationalRegistryStorage<S> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn slot(&self, id: usize) -> Option<&GenerationalSlot<S>> {
        self.states.get(id)
    }

    pub(crate) fn slot_mut(&mut self, id: usize) -> Option<&mut GenerationalSlot<S>> {
        self.states.get_mut(id)
    }

    pub(crate) fn slots(&self) -> impl Iterator<Item = (usize, &GenerationalSlot<S>)> + '_ {
        self.states.iter()
    }

    pub(crate) fn slot_len(&self) -> usize {
        self.states.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.states.capacity()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.states.capacity() * size_of::<Option<GenerationalSlot<S>>>()
    }

    pub(crate) fn clear(&mut self) {
        self.states.clear();
        self.active_count = 0;
        self.detached_count = 0;
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit();
    }

    // Gives back the room of a registry left at a quarter of its ids or
    // less, and says whether it did.
    pub(crate) fn maybe_shrink(&mut self) -> bool {
        if self.capacity() <= SHRINK_FLOOR || self.slot_len().saturating_mul(4) >= self.capacity() {
            return false;
        }
        self.shrink_to_fit();
        true
    }

    #[cfg(test)]
    pub(crate) fn dense_capacity(&self) -> usize {
        self.states.capacity()
    }

    fn insert_slot(&mut self, id: usize, generation: u32, state: S) -> Option<GenerationalSlot<S>> {
        self.states
            .insert(id, GenerationalSlot { generation, state })
    }

    fn remove_slot(&mut self, id: usize) -> Option<GenerationalSlot<S>> {
        self.states.remove(id)
    }
}

impl<S> GrowthSlack for GenerationalRegistryStorage<S> {
    fn trim_growth_slack(&mut self) {
        self.states.trim_growth_slack();
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
        if let Some(slot) = self.slot(id)
            && slot.generation != generation
        {
            return None;
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
        if previous.is_some_and(RegistryState::is_active) {
            self.active_count -= 1;
        }
        if previous.is_some_and(RegistryState::is_detached) {
            self.detached_count -= 1;
        }
        if next.is_some_and(RegistryState::is_active) {
            self.active_count += 1;
        }
        if next.is_some_and(RegistryState::is_detached) {
            self.detached_count += 1;
        }
    }
}
