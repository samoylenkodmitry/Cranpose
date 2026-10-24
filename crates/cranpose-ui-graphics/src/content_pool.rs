use std::{collections::HashMap, rc::Rc};

use crate::FxBuildHasher;

/// A value a [`ContentPool`] holds, which tells whether a holder outside the
/// pool keeps it.
pub trait PooledValue {
    /// Whether anything besides the pool holds the value.
    fn is_held(&self) -> bool;
}

impl<T: ?Sized> PooledValue for Rc<T> {
    fn is_held(&self) -> bool {
        Rc::strong_count(self) > 1
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
    values: HashMap<u64, V, FxBuildHasher>,
    sweep_at: usize,
}

impl<V: PooledValue> ContentPool<V> {
    /// An empty pool.
    pub const fn new() -> Self {
        Self {
            values: HashMap::with_hasher(FxBuildHasher::new()),
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
        if let Some(value) = self.values.get(&hash)
            && matches(value)
        {
            return read(value);
        }
        if self.values.len() >= self.sweep_at {
            self.values.retain(|_, value| value.is_held());
            self.sweep_at = (self.values.len() * 2).max(FIRST_SWEEP);
            self.values.shrink_to(self.sweep_at);
        }
        read(self.values.entry(hash).insert_entry(make()).get())
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
