use std::rc::{Rc, Weak};

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
    pub(crate) fn add<T: StateObject + 'static>(&mut self, state: &Rc<T>) {
        let hash = Rc::as_ptr(state) as *const () as usize;
        let trait_obj: Rc<dyn StateObject> = state.clone();
        let weak = Rc::downgrade(&trait_obj);

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

    pub(crate) fn add_trait_object(&mut self, state: &Rc<dyn StateObject>) {
        let hash = Rc::as_ptr(state) as *const () as usize;
        let weak = Rc::downgrade(state);

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
#[path = "tests/snapshot_weak_set_tests.rs"]
mod tests;
