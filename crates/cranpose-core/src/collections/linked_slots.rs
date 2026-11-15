//! The entries of a bounded cache: a slab found by key through a hash
//! index, each entry linked into one of the recency orders its cache keeps.

use std::{
    borrow::Borrow,
    hash::{BuildHasher, Hash},
    num::NonZeroUsize,
};

use hashbrown::HashTable;

use super::map::RandomState;

pub(super) struct LinkedSlot<K, V> {
    pub(super) key: K,
    pub(super) value: V,
    newer: Option<u32>,
    older: Option<u32>,
}

// The newest and the oldest entry of one recency order. A slot's order is
// known to its cache, which hands it to each link and unlink.
#[derive(Clone, Copy, Default)]
pub(super) struct Order {
    pub(super) newest: Option<u32>,
    pub(super) oldest: Option<u32>,
}

fn occupied<K, V>(slots: &[Option<LinkedSlot<K, V>>], slot: u32) -> &LinkedSlot<K, V> {
    slots[slot as usize]
        .as_ref()
        .expect("an indexed or linked cache slot is always occupied")
}

pub(super) struct LinkedSlots<K, V> {
    index: HashTable<u32>,
    hasher: RandomState,
    slots: Vec<Option<LinkedSlot<K, V>>>,
    free: Vec<u32>,
}

impl<K, V> LinkedSlots<K, V>
where
    K: Eq + Hash,
{
    pub(super) fn new() -> Self {
        Self {
            index: HashTable::new(),
            hasher: RandomState::default(),
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.index.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub(super) fn hash<Q>(&self, key: &Q) -> u64
    where
        Q: Hash + ?Sized,
    {
        self.hasher.hash_one(key)
    }

    pub(super) fn find<Q>(&self, key: &Q) -> Option<u32>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.find_hashed(self.hash(key), key)
    }

    // The slot of `key`, which `hash` is the hash of.
    pub(super) fn find_hashed<Q>(&self, hash: u64, key: &Q) -> Option<u32>
    where
        K: Borrow<Q>,
        Q: Eq + ?Sized,
    {
        let slots = &self.slots;
        self.index
            .find(hash, |&slot| occupied(slots, slot).key.borrow() == key)
            .copied()
    }

    pub(super) fn slot(&self, slot: u32) -> &LinkedSlot<K, V> {
        occupied(&self.slots, slot)
    }

    pub(super) fn slot_mut(&mut self, slot: u32) -> &mut LinkedSlot<K, V> {
        self.slots[slot as usize]
            .as_mut()
            .expect("a linked cache slot is always occupied")
    }

    // Stores `key`, which `hash` is the hash of and the slots lack, with
    // `value` in a free slot, linked into no order yet, for a cache bounded
    // to `cap` entries.
    pub(super) fn insert(&mut self, hash: u64, (key, value): (K, V), cap: NonZeroUsize) -> u32 {
        let slot = self.claim_slot(key, value, cap);
        let (slots, hasher) = (&self.slots, &self.hasher);
        self.index.insert_unique(hash, slot, |&slot| {
            hasher.hash_one(&occupied(slots, slot).key)
        });
        slot
    }

    // Takes the entry of `slot` out of `order` and of the slots, with the
    // hash of its key.
    pub(super) fn remove(&mut self, slot: u32, order: &mut Order) -> (u64, LinkedSlot<K, V>) {
        let hash = self.hash(&self.slot(slot).key);
        if let Ok(entry) = self.index.find_entry(hash, |&indexed| indexed == slot) {
            entry.remove();
        }
        self.unlink(slot, order);
        (hash, self.release_slot(slot))
    }

    // Takes the entry under `key` out of `order` and of the slots.
    pub(super) fn take<Q>(&mut self, key: &Q, order: &mut Order) -> Option<LinkedSlot<K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let hash = self.hash(key);
        let slots = &self.slots;
        let (slot, _) = self
            .index
            .find_entry(hash, |&slot| occupied(slots, slot).key.borrow() == key)
            .ok()?
            .remove();
        self.unlink(slot, order);
        Some(self.release_slot(slot))
    }

    // Makes `slot`, linked into `order`, the newest of it.
    pub(super) fn promote(&mut self, slot: u32, order: &mut Order) {
        if order.newest == Some(slot) {
            return;
        }
        self.unlink(slot, order);
        self.link_newest(slot, order);
    }

    pub(super) fn link_newest(&mut self, slot: u32, order: &mut Order) {
        let previous_newest = order.newest;
        {
            let entry = self.slot_mut(slot);
            entry.newer = None;
            entry.older = previous_newest;
        }
        if let Some(previous) = previous_newest {
            self.slot_mut(previous).newer = Some(slot);
        }
        order.newest = Some(slot);
        if order.oldest.is_none() {
            order.oldest = Some(slot);
        }
    }

    pub(super) fn unlink(&mut self, slot: u32, order: &mut Order) {
        let (newer, older) = {
            let entry = self.slot_mut(slot);
            (entry.newer.take(), entry.older.take())
        };
        match newer {
            Some(newer) => self.slot_mut(newer).older = older,
            None => order.newest = older,
        }
        match older {
            Some(older) => self.slot_mut(older).newer = newer,
            None => order.oldest = newer,
        }
    }

    // Moves every entry of `from` into `to`, newer than all of `to`'s, in
    // the order they had.
    pub(super) fn append(&mut self, to: &mut Order, from: &mut Order) {
        let Some(first) = from.oldest else {
            return;
        };
        match to.newest {
            Some(last) => {
                self.slot_mut(last).newer = Some(first);
                self.slot_mut(first).older = Some(last);
            }
            None => to.oldest = Some(first),
        }
        to.newest = from.newest;
        *from = Order::default();
    }

    // The entries of `order`, newest first.
    pub(super) fn iter(&self, order: Order) -> impl Iterator<Item = (&K, &V)> {
        let mut next = order.newest;
        std::iter::from_fn(move || {
            let entry = self.slot(next?);
            next = entry.older;
            Some((&entry.key, &entry.value))
        })
    }

    fn claim_slot(&mut self, key: K, value: V, cap: NonZeroUsize) -> u32 {
        let entry = LinkedSlot {
            key,
            value,
            newer: None,
            older: None,
        };
        match self.free.pop() {
            Some(slot) => {
                self.slots[slot as usize] = Some(entry);
                slot
            }
            None => {
                let slot =
                    u32::try_from(self.slots.len()).expect("a cache holds fewer than 2^32 entries");
                // A full cache keeps its slots, so growth adds a quarter of
                // them, where doubling would leave room the bound may never
                // let entries take.
                let len = self.slots.len();
                if len == self.slots.capacity() {
                    let room = cap.get().saturating_sub(len);
                    self.slots.reserve_exact((len / 4).max(8).min(room).max(1));
                }
                self.slots.push(Some(entry));
                slot
            }
        }
    }

    fn release_slot(&mut self, slot: u32) -> LinkedSlot<K, V> {
        let entry = self.slots[slot as usize]
            .take()
            .expect("a slot being released is always occupied");
        self.free.push(slot);
        entry
    }
}

#[cfg(test)]
#[path = "tests/linked_slots_tests.rs"]
mod tests;
