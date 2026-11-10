use std::{collections::hash_map::Entry, rc::Rc, sync::Arc};

use crate::{FxBuildHasher, FxHashMap};

/// A value a [`ContentPool`] holds, which tells whether a holder outside the
/// pool keeps it.
pub trait PooledValue {
    /// Whether anything besides the pool holds the value.
    fn is_held(&self) -> bool;
}

impl<T: ?Sized> PooledValue for Rc<T> {
    fn is_held(&self) -> bool {
        Self::strong_count(self) > 1
    }
}

impl<T: ?Sized> PooledValue for Arc<T> {
    fn is_held(&self) -> bool {
        Self::strong_count(self) > 1
    }
}

/// The values a pool takes before its first sweep.
const FIRST_SWEEP: usize = 16;

/// Values shared by their content, as the text a scene draws frame after
/// frame: a lookup hands out the value an earlier lookup made for the same
/// content, so a label drawn again takes no new copy.
///
/// Before an insertion would take the pool to twice the values its last
/// sweep kept, it drops the values nothing else holds. It keeps what the
/// screen shows, then: a text that changes every frame leaves once the
/// frames that drew it are gone, on any platform's memory.
pub struct ContentPool<V> {
    values: FxHashMap<u64, V>,
    sweep_at: usize,
}

impl<V: PooledValue> ContentPool<V> {
    /// An empty pool.
    pub const fn new() -> Self {
        Self {
            values: FxHashMap::with_hasher(FxBuildHasher::new()),
            sweep_at: FIRST_SWEEP,
        }
    }

    /// The number of values the pool holds.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the pool holds no value.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// `read` of the value under `hash` that `matches` accepts, which `make`
    /// makes when the pool holds none. A value under the same hash that
    /// `matches` refuses, another content's, gives way to the new one.
    pub fn with<R>(
        &mut self,
        hash: u64,
        matches: impl FnOnce(&V) -> bool,
        make: impl FnOnce() -> V,
        read: impl FnOnce(&V) -> R,
    ) -> R {
        // One probe finds the value or the room for it: a text that changes
        // every frame misses each time.
        let full = self.values.len() >= self.sweep_at;
        match self.values.entry(hash) {
            Entry::Occupied(mut entry) => {
                if !matches(entry.get()) {
                    entry.insert(make());
                }
                return read(entry.get());
            }
            Entry::Vacant(entry) if !full => return read(entry.insert(make())),
            Entry::Vacant(_) => {}
        }
        self.sweep();
        read(self.values.entry(hash).insert_entry(make()).get())
    }

    // Drops the values nothing holds. Out of line: a pool sweeps once its
    // size doubles, and each caller of `with` would grow by the sweep.
    #[inline(never)]
    fn sweep(&mut self) {
        self.values.retain(|_, value| value.is_held());
        self.sweep_at = (self.values.len() * 2).max(FIRST_SWEEP);
        self.values.shrink_to(self.sweep_at);
    }
}

impl<V: PooledValue> Default for ContentPool<V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "tests/content_pool_tests.rs"]
mod tests;
