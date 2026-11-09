//! Where each stable node id's node lives in the applier's dense storage, and
//! how many times that id's storage was recycled.
//!
//! Stable ids come from a counter, so live ids cluster and retire roughly in
//! the order they were made, which [`IdPages`] holds: a page is freed once
//! none of its ids has a slot or a generation.

use crate::{
    NodeId,
    collections::id_pages::{IdPages, Vacant},
};

// The bit of a generation that says the id's node is virtual, set while the
// id has a slot: a walk up the tree finds it with the slot it looks up.
const VIRTUAL: u32 = 1 << 31;
const NO_SLOT: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Entry {
    slot: u32,
    generation: u32,
}

impl Entry {
    const EMPTY: Self = Self {
        slot: NO_SLOT,
        generation: 0,
    };

    fn has_slot(self) -> bool {
        self.slot != NO_SLOT
    }
}

impl Vacant for Entry {
    fn vacant() -> Self {
        Self::EMPTY
    }

    fn is_vacant(&self) -> bool {
        !self.has_slot() && self.generation == 0
    }
}

#[derive(Default)]
pub(crate) struct StableIndex {
    entries: IdPages<Entry>,
    live: usize,
}

impl StableIndex {
    #[inline]
    fn entry(&self, id: NodeId) -> Entry {
        self.entries.entry(id.0).copied().unwrap_or(Entry::EMPTY)
    }

    /// The dense storage slot `id`'s node sits in.
    #[inline]
    pub(crate) fn slot(&self, id: NodeId) -> Option<usize> {
        let slot = self.entry(id).slot;
        (slot != NO_SLOT).then_some(slot as usize)
    }

    // The dense storage slot `id`'s node sits in, and whether that node is
    // virtual.
    #[inline]
    pub(crate) fn slot_and_virtual(&self, id: NodeId) -> Option<(usize, bool)> {
        let entry = self.entry(id);
        entry
            .has_slot()
            .then_some((entry.slot as usize, entry.generation & VIRTUAL != 0))
    }

    /// How many times `id`'s node left the tree; `0` for an id never seen.
    pub(crate) fn generation(&self, id: NodeId) -> u32 {
        self.entry(id).generation & !VIRTUAL
    }

    /// Puts `id`'s node in dense storage `slot`, keeping its generation.
    pub(crate) fn set_slot(&mut self, id: NodeId, slot: usize) {
        debug_assert!(slot < NO_SLOT as usize, "dense slot {slot} past the index");
        self.update(id, |entry| entry.slot = slot as u32);
    }

    /// Puts a newly made `id`'s node in dense storage `slot`, at generation 0.
    pub(crate) fn insert_fresh(&mut self, id: NodeId, slot: usize) {
        debug_assert!(slot < NO_SLOT as usize, "dense slot {slot} past the index");
        self.update(id, |entry| {
            entry.slot = slot as u32;
            entry.generation = 0;
        });
    }

    // Notes that the node `id`'s slot holds is virtual, until it leaves.
    pub(crate) fn mark_virtual(&mut self, id: NodeId) {
        self.update(id, |entry| {
            debug_assert!(entry.has_slot(), "only a stored node is virtual");
            entry.generation |= VIRTUAL;
        });
    }

    /// Takes `id`'s node out of dense storage and counts one more generation.
    pub(crate) fn release(&mut self, id: NodeId) {
        self.update(id, |entry| {
            entry.slot = NO_SLOT;
            entry.generation = (entry.generation & !VIRTUAL).wrapping_add(1) & !VIRTUAL;
        });
    }

    /// Forgets the generations of ids without a node for which `keep` says no.
    pub(crate) fn retain_retired(&mut self, keep: impl Fn(NodeId) -> bool) {
        self.entries
            .retain(|id, entry| entry.has_slot() || keep(NodeId(id)));
    }

    /// Ids with a node in dense storage.
    pub(crate) fn live(&self) -> usize {
        self.live
    }

    /// Ids with a node or a generation.
    pub(crate) fn occupied(&self) -> usize {
        self.entries.occupied()
    }

    /// Ids the allocated pages can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// Changes `id`'s entry and counts the ids with a node.
    fn update(&mut self, id: NodeId, change: impl FnOnce(&mut Entry)) {
        let mut had_slot = false;
        let mut has_slot = false;
        self.entries.update(id.0, |entry| {
            had_slot = entry.has_slot();
            change(entry);
            has_slot = entry.has_slot();
        });
        self.live = self.live + usize::from(has_slot) - usize::from(had_slot);
    }
}

#[cfg(test)]
#[path = "tests/stable_index_tests.rs"]
mod tests;
