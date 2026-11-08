// Values of one kind a store keeps by index, the index of a removed one
// going to the next one put in.

use std::ops::{Index, IndexMut};

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

    pub(crate) fn remove(&mut self, index: usize) -> Option<T> {
        let value = self.entries.get_mut(index)?.take()?;
        self.free
            .push(u32::try_from(index).expect("fewer than 2^32 entries"));
        Some(value)
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.free.clear();
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
