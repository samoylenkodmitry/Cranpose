//! Caches that forget what layout stopped asking for.

use std::{borrow::Borrow, hash::Hash, num::NonZeroUsize};

use super::{
    linked_slots::{LinkedSlots, Order},
    recent_hashes::RecentHashes,
};

/// Layout passes an entry may go without a lookup before it leaves, with
/// memory to spare. Text on screen is measured again only when its layout
/// changes, so this is how long a list may keep an item scrolled out before
/// measuring it anew costs a miss.
pub const IDLE_PASSES: u64 = 120;

/// A value and the layout pass that last looked it up.
struct Used<V> {
    value: V,
    pass: u64,
    // Whether pass `pass` asked for it again, or pushed it again after it
    // left for room: it then outranks the entries asked for once.
    again: bool,
}

/// A bounded cache whose entries also leave once a number of layout passes,
/// such as [`IDLE_PASSES`], go by without a lookup of them. A scrolling list
/// then keeps the measurements of the texts it shows, not of every text it
/// has shown up to the bound.
///
/// Within a pass, an entry the pass asks for again, or pushes again after
/// it left for room, outranks the entries the pass asked for once, which a
/// full cache lets go first, the least recently used first. A pass that
/// measures more texts than the cache holds, such as a grid of thousands of
/// cells, then keeps the texts that recur among the ones it measures once.
/// Across passes the cache keeps the most recently used entries.
pub struct PassAgedCache<K, V> {
    slots: LinkedSlots<K, Used<V>>,
    // The entries a full cache lets go first: those this pass asked for
    // once, and those of earlier passes, which are older.
    once: Order,
    // The entries this pass asked for again.
    again: Order,
    // The keys that left `once` for room, so one that comes back is known.
    left: RecentHashes,
    cap: NonZeroUsize,
    pass: u64,
}

impl<K, V> PassAgedCache<K, V>
where
    K: Eq + Hash,
{
    /// Creates an empty cache bounded to `capacity` entries, or to one when `capacity` is zero.
    pub fn with_capacity_at_least_one(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        Self {
            slots: LinkedSlots::new(),
            once: Order::default(),
            again: Order::default(),
            left: RecentHashes::new(cap.get()),
            cap,
            pass: 0,
        }
    }

    /// Returns the number of entries the cache holds.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns `true` if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Drops every entry and gives back the storage they took.
    pub fn clear(&mut self) {
        self.slots = LinkedSlots::new();
        self.once = Order::default();
        self.again = Order::default();
        self.left.clear();
    }

    /// The value under `key`, marked used in the current pass.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.get_mut(key).map(|value| &*value)
    }

    /// The value under `key` to change in place, marked used in the current
    /// pass.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let slot = self.slots.find(key)?;
        Some(self.use_again(slot))
    }

    // Marks the entry of `slot` asked for again in the current pass, the
    // newest of those, and returns its value.
    fn use_again(&mut self, slot: u32) -> &mut V {
        let pass = self.pass;
        let used = &self.slots.slot(slot).value;
        if used.again && used.pass == pass {
            self.slots.promote(slot, &mut self.again);
        } else {
            self.slots.unlink(slot, &mut self.once);
            self.slots.link_newest(slot, &mut self.again);
        }
        let used = &mut self.slots.slot_mut(slot).value;
        used.pass = pass;
        used.again = true;
        &mut used.value
    }

    /// Stores `value` under `key`, returning the entry it replaced or the
    /// one it pushed out of a full cache.
    pub fn push(&mut self, key: K, value: V) -> Option<(K, V)> {
        let hash = self.slots.hash(&key);
        if let Some(slot) = self.slots.find_hashed(hash, &key) {
            let old = std::mem::replace(self.use_again(slot), value);
            return Some((key, old));
        }
        // Read before the eviction, whose key may take the same tag slot.
        let again = self.left.contains(hash);
        let evicted = if self.slots.len() >= self.cap.get() {
            self.evict()
        } else {
            None
        };
        let used = Used {
            value,
            pass: self.pass,
            again,
        };
        let slot = self.slots.insert(hash, (key, used), self.cap);
        let order = if again {
            &mut self.again
        } else {
            &mut self.once
        };
        self.slots.link_newest(slot, order);
        evicted
    }

    // Lets the least recently used entry asked for once go, remembering its
    // key, or the least recently used one asked for again when there is
    // none. Out of line, as only a full cache gets here.
    #[inline(never)]
    fn evict(&mut self) -> Option<(K, V)> {
        if let Some(slot) = self.once.oldest {
            let (hash, entry) = self.slots.remove(slot, &mut self.once);
            self.left.put(hash);
            return Some((entry.key, entry.value.value));
        }
        let slot = self.again.oldest?;
        let (_, entry) = self.slots.remove(slot, &mut self.again);
        Some((entry.key, entry.value.value))
    }

    /// Starts a layout pass: the entries the last pass asked for again
    /// become the newest of those asked for once, and the entries no lookup
    /// asked for in the last `idle_passes` passes leave, least recently used
    /// first, each value handed to `dropped`. It stops at the first entry
    /// still in use, so it costs one comparison when nothing has gone idle.
    pub fn begin_pass(&mut self, idle_passes: u64, mut dropped: impl FnMut(V)) {
        self.pass += 1;
        let pass = self.pass;
        self.slots.append(&mut self.once, &mut self.again);
        while let Some(slot) = self.once.oldest
            && pass - self.slots.slot(slot).value.pass > idle_passes
        {
            let (_, entry) = self.slots.remove(slot, &mut self.once);
            dropped(entry.value.value);
        }
    }
}

#[cfg(test)]
#[path = "tests/pass_aged_tests.rs"]
mod tests;
