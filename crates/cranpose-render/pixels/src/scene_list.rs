//! A list of a raster scene. Where a `Vec` doubles, it grows by a quarter of
//! its length, at least by `MIN_GROWTH` items: a scene keeps its storage for
//! as long as its screen, so the room a doubling leaves past its items stays
//! taken, and while a list grows its old and new storage live at once.

use std::{
    ops::{Deref, DerefMut, Range},
    vec::Drain,
};

const MIN_GROWTH: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct SceneList<T>(Vec<T>);

impl<T> SceneList<T> {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn push(&mut self, item: T) {
        self.reserve(1);
        self.0.push(item);
    }

    pub(crate) fn extend_from_slice(&mut self, items: &[T])
    where
        T: Clone,
    {
        self.reserve(items.len());
        self.0.extend_from_slice(items);
    }

    pub(crate) fn extend(&mut self, items: impl ExactSizeIterator<Item = T>) {
        self.reserve(items.len());
        self.0.extend(items);
    }

    // Puts `items` in place of the items of `range`.
    pub(crate) fn splice(&mut self, range: Range<usize>, items: impl ExactSizeIterator<Item = T>) {
        self.reserve(items.len().saturating_sub(range.len()));
        self.0.splice(range, items);
    }

    // Takes every item out, keeping the storage.
    pub(crate) fn drain(&mut self) -> Drain<'_, T> {
        self.0.drain(..)
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    // Makes room for `additional` more items. Only the test is inlined: a
    // push runs on every frame that draws, and growth seldom.
    #[inline]
    fn reserve(&mut self, additional: usize) {
        if self.0.capacity() - self.0.len() < additional {
            self.grow(additional);
        }
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self, additional: usize) {
        self.0
            .reserve_exact(additional.max(self.0.len() / 4).max(MIN_GROWTH));
    }
}

impl<T> Default for SceneList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Deref for SceneList<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.0
    }
}

impl<T> DerefMut for SceneList<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<'a, T> IntoIterator for &'a SceneList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
