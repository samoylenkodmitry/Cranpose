//! A cache that keeps the most recently used values, up to a capacity.

use std::{collections::HashMap, hash::Hash};

pub struct Recent<K, V> {
    capacity: usize,
    tick: u64,
    entries: HashMap<K, (V, u64)>,
}

impl<K: Hash + Eq + Clone, V> Recent<K, V> {
    /// A cache that holds at most `capacity` values; one of capacity 0
    /// keeps nothing.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    /// The value of `key`, which becomes the most recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(value, used)| {
            *used = tick;
            &*value
        })
    }

    /// Stores `value` under `key` and returns the value it replaced. When the
    /// cache is full and `key` is new, the least recently used value goes.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.capacity == 0 {
            return None;
        }
        self.tick += 1;
        if !self.entries.contains_key(&key) && self.entries.len() == self.capacity {
            self.evict_oldest();
        }
        self.entries
            .insert(key, (value, self.tick))
            .map(|(old, _)| old)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_oldest(&mut self) {
        let oldest = self
            .entries
            .iter()
            .min_by_key(|(_, (_, used))| *used)
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.entries.remove(&key);
        }
    }
}
