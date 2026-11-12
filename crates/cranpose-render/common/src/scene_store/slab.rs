// Values of one kind a store keeps by index, the index of a removed one
// going to the next one put in.

use std::ops::{Index, IndexMut};

use cranpose_core::collections::growth_slack::GrowthSlack;

#[derive(Clone)]
pub(crate) struct Slab<T> {
    entries: Vec<Option<T>>,
    free: Vec<u32>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Slab<T> {
    pub(crate) fn insert(&mut self, value: T) -> usize {
        match self.free.pop() {
            Some(index) => {
                let index = index as usize;
                self.entries[index] = Some(value);
                index
            }
            None => {
                self.entries.push(Some(value));
                self.entries.len() - 1
            }
        }
    }

    // Drops the entry at `index` where it lies, which a move out would copy
    // first.
    pub(crate) fn remove(&mut self, index: usize) {
        let Some(entry) = self.entries.get_mut(index) else {
            return;
        };
        if entry.is_none() {
            return;
        }
        *entry = None;
        self.free
            .push(u32::try_from(index).expect("fewer than 2^32 entries"));
    }
    // Takes the entry at `index` out; its index goes to the next one put in.
    pub(crate) fn take(&mut self, index: usize) -> Option<T> {
        let value = self.entries.get_mut(index)?.take()?;
        self.free
            .push(u32::try_from(index).expect("fewer than 2^32 entries"));
        Some(value)
    }

    // Takes the entry at `index` out while its index stays its own: no
    // other entry takes the index before `put` fills it again.
    pub(crate) fn lend(&mut self, index: usize) -> Option<T> {
        self.entries.get_mut(index)?.take()
    }

    // Fills the entry at `index`, which `lend` took out.
    pub(crate) fn put(&mut self, index: usize, value: T) {
        self.entries[index] = Some(value);
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().flatten()
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.entries.iter_mut().flatten()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Option<T>>() + self.free.capacity() * size_of::<u32>()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.free.clear();
    }

    pub(crate) fn trim_growth_slack(&mut self) {
        self.entries.trim_growth_slack();
    }

    // Cuts the empty entries at the end, so the room they took can go.
    pub(crate) fn cut_free_tail(&mut self) {
        let len = self
            .entries
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |last| last + 1);
        if len < self.entries.len() {
            // The cut entries are empty: a truncate would drop each of them
            // with the drop code of the entry type, which every slab would
            // repeat.
            std::mem::forget(self.entries.drain(len..));
            cut_free_list(&mut self.free, len);
        }
    }
}

// Drops the free indices a cut took the entries of. Out of line, one copy
// for every slab: a trim runs at rest.
#[inline(never)]
fn cut_free_list(free: &mut Vec<u32>, len: usize) {
    free.retain(|&index| (index as usize) < len);
    free.trim_growth_slack();
}

impl<T> Index<usize> for Slab<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        self.entries[index]
            .as_ref()
            .expect("a live handle names a present entry")
    }
}

impl<T> IndexMut<usize> for Slab<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.entries[index]
            .as_mut()
            .expect("a live handle names a present entry")
    }
}
