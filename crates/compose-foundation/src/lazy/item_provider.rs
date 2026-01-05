//! Item provider trait for lazy layouts.
//!
//! This module defines the [`LazyLayoutItemProvider`] trait which provides
//! all needed information about items for lazy composition and measurement.

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
    ///
    /// Content types are represented as `u64` for efficient comparison and storage.
    fn get_content_type(&self, index: usize) -> Option<u64> {
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
