// An array of the store that notes when its length changes: only a change
// of length can leave room to give back, so the room is checked after one
// alone. Its entries read and change in place as a slice's.

use std::ops::{Deref, DerefMut};

use cranpose_core::collections::growth_slack::GrowthSlack;

#[derive(Clone)]
pub(super) struct ResizeVec<T> {
    values: Vec<T>,
    resized: bool,
}

impl<T> Default for ResizeVec<T> {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            resized: false,
        }
    }
}

impl<T> ResizeVec<T> {
    pub(super) fn push(&mut self, value: T) {
        self.values.push(value);
        self.resized = true;
    }

    pub(super) fn resize_with(&mut self, len: usize, value: impl FnMut() -> T) {
        self.values.resize_with(len, value);
        self.resized = true;
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.resized = true;
    }

    pub(super) fn truncate(&mut self, len: usize) {
        self.values.truncate(len);
        self.resized = true;
    }

    // Notes a change that may leave room to give back, such as the removal
    // of the record of the last row.
    pub(super) fn note_resized(&mut self) {
        self.resized = true;
    }

    pub(super) fn capacity(&self) -> usize {
        self.values.capacity()
    }

    // Whether the length changed since the room was last checked.
    #[inline]
    pub(super) fn resized(&self) -> bool {
        self.resized
    }

    pub(super) fn trim_growth_slack(&mut self) {
        if std::mem::take(&mut self.resized) {
            self.values.trim_growth_slack();
        }
    }
}

impl<T: Clone> ResizeVec<T> {
    pub(super) fn resize(&mut self, len: usize, value: T) {
        self.values.resize(len, value);
        self.resized = true;
    }
}

impl<T> Deref for ResizeVec<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.values
    }
}

impl<T> DerefMut for ResizeVec<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
