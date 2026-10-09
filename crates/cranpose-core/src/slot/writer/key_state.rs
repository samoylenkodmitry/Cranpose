use super::super::GroupKey;
use crate::{
    Key,
    collections::map::{HashMap, HashSet},
};

const INLINE_KEY_STATE_CAPACITY: usize = 8;

/// Up to [`INLINE_KEY_STATE_CAPACITY`] items in place: clearing a recycled
/// frame's list only resets its length.
struct InlineList<T: Copy> {
    items: [T; INLINE_KEY_STATE_CAPACITY],
    len: usize,
}

impl<T: Copy> InlineList<T> {
    fn new(fill: T) -> Self {
        Self {
            items: [fill; INLINE_KEY_STATE_CAPACITY],
            len: 0,
        }
    }

    fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len]
    }

    /// Appends `item`, or returns `false` when the list is full.
    fn push(&mut self, item: T) -> bool {
        let Some(slot) = self.items.get_mut(self.len) else {
            return false;
        };
        *slot = item;
        self.len += 1;
        true
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    /// Empties the list and returns what it held.
    fn take(&mut self) -> &[T] {
        let len = std::mem::take(&mut self.len);
        &self.items[..len]
    }
}

/// The child keys a group frame has seen: in place while few, in a hash map
/// or set once more distinct keys arrive. A cleared frame starts in place
/// again and keeps the map's storage for the next spill.
pub(in crate::slot) struct FrameKeyState {
    ordinals: InlineList<(Key, u32)>,
    ordinal_map: HashMap<Key, u32>,
    ordinals_spilled: bool,
    seen: InlineList<GroupKey>,
    seen_set: HashSet<GroupKey>,
    seen_spilled: bool,
}

impl Default for FrameKeyState {
    fn default() -> Self {
        Self {
            ordinals: InlineList::new((0, 0)),
            ordinal_map: HashMap::default(),
            ordinals_spilled: false,
            seen: InlineList::new(GroupKey::new(0, None, 0)),
            seen_set: HashSet::default(),
            seen_spilled: false,
        }
    }
}

impl FrameKeyState {
    pub(in crate::slot) fn clear(&mut self) {
        self.ordinals.clear();
        if self.ordinals_spilled {
            self.ordinal_map.clear();
            self.ordinals_spilled = false;
        }
        self.seen.clear();
        if self.seen_spilled {
            self.seen_set.clear();
            self.seen_spilled = false;
        }
    }

    pub(in crate::slot) fn expected_ordinal(&self, key: Key) -> u32 {
        if self.ordinals_spilled {
            return self.ordinal_map.get(&key).copied().unwrap_or(0);
        }
        self.ordinals
            .as_slice()
            .iter()
            .find_map(|(entry_key, ordinal)| (*entry_key == key).then_some(*ordinal))
            .unwrap_or(0)
    }

    pub(in crate::slot) fn next_ordinal(&mut self, key: Key) -> u32 {
        if self.ordinals_spilled {
            let ordinal = self.ordinal_map.entry(key).or_insert(0);
            let current = *ordinal;
            *ordinal += 1;
            return current;
        }
        if let Some((_, ordinal)) = self
            .ordinals
            .as_mut_slice()
            .iter_mut()
            .find(|(entry_key, _)| *entry_key == key)
        {
            let current = *ordinal;
            *ordinal += 1;
            return current;
        }
        if !self.ordinals.push((key, 1)) {
            self.spill_ordinals().insert(key, 1);
        }
        0
    }

    pub(in crate::slot) fn insert_seen(&mut self, key: GroupKey) -> bool {
        if self.seen_spilled {
            return self.seen_set.insert(key);
        }
        if self.seen.as_slice().contains(&key) {
            return false;
        }
        if !self.seen.push(key) {
            self.spill_seen().insert(key);
        }
        true
    }

    fn spill_ordinals(&mut self) -> &mut HashMap<Key, u32> {
        let entries = self.ordinals.take();
        self.ordinal_map.reserve(entries.len() * 2);
        self.ordinal_map.extend(entries.iter().copied());
        self.ordinals_spilled = true;
        &mut self.ordinal_map
    }

    fn spill_seen(&mut self) -> &mut HashSet<GroupKey> {
        let entries = self.seen.take();
        self.seen_set.reserve(entries.len() * 2);
        self.seen_set.extend(entries.iter().copied());
        self.seen_spilled = true;
        &mut self.seen_set
    }

    #[cfg(test)]
    fn ordinals_are_promoted(&self) -> bool {
        self.ordinals_spilled
    }

    #[cfg(test)]
    fn seen_is_promoted(&self) -> bool {
        self.seen_spilled
    }
}

#[cfg(test)]
#[path = "tests/key_state_tests.rs"]
mod tests;
