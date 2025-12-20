//! Item provider trait for lazy layouts.
//!
//! This module defines the [`LazyLayoutItemProvider`] trait which provides
//! all needed information about items for lazy composition and measurement.

use std::any::Any;

/// Provides all the needed info about items which could be composed and
/// measured by lazy layouts.
///
/// This follows the Jetpack Compose `LazyLayoutItemProvider` pattern.
/// Implementations should be immutable - changes to the data source
/// should create a new provider instance.
pub trait LazyLayoutItemProvider {
    /// The total number of items in the lazy layout (visible or not).
    fn item_count(&self) -> usize;

    /// Returns the key for the item at the given index.
    ///
    /// Keys are used to:
    /// - Maintain scroll position when items are added/removed
    /// - Efficiently diff items during recomposition
    /// - Enable item animations
    ///
    /// If not overridden, defaults to the index itself.
    fn get_key(&self, index: usize) -> u64 {
        index as u64
    }

    /// Returns the content type for the item at the given index.
    ///
    /// Items with the same content type can be reused more efficiently.
    /// Returns `None` for items with no specific type (compatible with any).
    fn get_content_type(&self, index: usize) -> Option<&dyn Any> {
        let _ = index;
        None
    }

    /// Get the index for a given key.
    ///
    /// Used to find items by key for scroll-to operations.
    /// Returns `None` if the key is not found.
    fn get_index(&self, key: u64) -> Option<usize> {
        // Default implementation: linear search using iterator
        (0..self.item_count()).find(|&i| self.get_key(i) == key)
    }
}

/// Type alias for key function to reduce complexity.
pub type KeyFn<T> = Box<dyn Fn(&T, usize) -> u64>;

/// A simple item provider backed by a vector of items.
pub struct VecItemProvider<T> {
    items: Vec<T>,
    key_fn: Option<KeyFn<T>>,
}

impl<T> VecItemProvider<T> {
    /// Creates a new provider from a vector of items.
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            key_fn: None,
        }
    }

    /// Creates a new provider with a custom key function.
    pub fn with_key_fn<F>(items: Vec<T>, key_fn: F) -> Self
    where
        F: Fn(&T, usize) -> u64 + 'static,
    {
        Self {
            items,
            key_fn: Some(Box::new(key_fn)),
        }
    }

    /// Gets a reference to the item at the given index.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    /// Gets the underlying items.
    pub fn items(&self) -> &[T] {
        &self.items
    }
}

impl<T> LazyLayoutItemProvider for VecItemProvider<T> {
    fn item_count(&self) -> usize {
        self.items.len()
    }

    fn get_key(&self, index: usize) -> u64 {
        if let Some(key_fn) = &self.key_fn {
            if let Some(item) = self.items.get(index) {
                return key_fn(item, index);
            }
        }
        index as u64
    }
}

/// Finds the position of an item with the given key.
///
/// This logic allows detection of items added/removed before the current
/// first visible item, enabling proper scroll position maintenance.
pub fn find_index_by_key<P: LazyLayoutItemProvider>(
    provider: &P,
    key: Option<u64>,
    last_known_index: usize,
) -> usize {
    let Some(key) = key else {
        return last_known_index;
    };

    if provider.item_count() == 0 {
        return last_known_index;
    }

    // Check if item is still at the same index
    if last_known_index < provider.item_count() && provider.get_key(last_known_index) == key {
        return last_known_index;
    }

    // Search for the new index
    provider.get_index(key).unwrap_or(last_known_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec_item_provider_count() {
        let provider = VecItemProvider::new(vec![1, 2, 3, 4, 5]);
        assert_eq!(provider.item_count(), 5);
    }

    #[test]
    fn test_vec_item_provider_default_key() {
        let provider = VecItemProvider::new(vec!["a", "b", "c"]);
        assert_eq!(provider.get_key(0), 0);
        assert_eq!(provider.get_key(1), 1);
        assert_eq!(provider.get_key(2), 2);
    }

    #[test]
    fn test_vec_item_provider_custom_key() {
        let provider = VecItemProvider::with_key_fn(vec![100, 200, 300], |item, _| *item as u64);
        assert_eq!(provider.get_key(0), 100);
        assert_eq!(provider.get_key(1), 200);
        assert_eq!(provider.get_key(2), 300);
    }

    #[test]
    fn test_find_index_by_key() {
        let provider = VecItemProvider::with_key_fn(vec![10, 20, 30], |item, _| *item as u64);

        // Key at same position
        assert_eq!(find_index_by_key(&provider, Some(10), 0), 0);

        // Key moved
        assert_eq!(find_index_by_key(&provider, Some(30), 0), 2);

        // Key not found, falls back to last known
        assert_eq!(find_index_by_key(&provider, Some(999), 1), 1);
    }
}
