// Values of one kind a store keeps by index, the index of a removed one
// going to the next one put in.

use std::ops::{Index, IndexMut};

use cranpose_core::collections::growth_slack::GrowthSlack;

#[derive(Clone)]
pub(crate) struct Slab<T> {
    entries: Vec<Option<T>>,
    free: Vec<u32>,
    // Whether the entries changed length since their room was last
    // checked: only a change of length can leave room to give back.
    resized: bool,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
            resized: false,
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
                self.resized = true;
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
    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        self.entries.get(index)?.as_ref()
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
        self.resized = true;
    }

    pub(crate) fn trim_growth_slack(&mut self) {
        if std::mem::take(&mut self.resized) {
            self.entries.trim_growth_slack();
        }
    }
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
