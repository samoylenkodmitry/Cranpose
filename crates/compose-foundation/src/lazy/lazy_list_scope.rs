//! DSL scope for building lazy list content.
//!
//! Provides [`LazyListScope`] trait and implementation for the ergonomic
//! `item {}` / `items {}` API used in `LazyColumn` and `LazyRow`.
//!
//! Based on JC's `LazyLayoutIntervalContent` pattern.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Key type for lazy list items.
///
/// Separates user-provided keys from default index-based keys to prevent collisions.
/// This matches JC's `getDefaultLazyLayoutKey()` pattern where a wrapper type
/// (`DefaultLazyKey`) ensures default keys never collide with user-provided keys.
///
/// # JC Reference
/// - `LazyLayoutIntervalContent.getKey()` returns `content.key?.invoke(localIndex) ?: getDefaultLazyLayoutKey(index)`
/// - `Lazy.android.kt` defines `DefaultLazyKey(index)` as a wrapper data class
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LazyLayoutKey {
    /// User-provided key (from `scope.item(key: Some(k), ...)` or `scope.items(key: Some(|i| ...), ...)`)
    User(u64),
    /// Default key based on global index. Cannot collide with User keys due to enum separation.
    Index(usize),
}

impl LazyLayoutKey {
    /// Converts to u64 for slot ID usage, ensuring no collision between User and Index keys.
    ///
    /// User keys retain their value directly.
    /// Index keys are tagged with a high bit to ensure they never overlap with user keys
    /// in the u64 space (users typically use small positive integers or hashes).
    #[inline]
    pub fn to_slot_id(self) -> u64 {
        match self {
            LazyLayoutKey::User(k) => k,
            // Use high bit (bit 63) to separate index keys from user keys.
            // This ensures Index(0) != User(0), Index(1) != User(1), etc.
            LazyLayoutKey::Index(i) => (1u64 << 63) | (i as u64),
        }
    }

    /// Returns true if this is a user-provided key.
    #[inline]
    pub fn is_user_key(self) -> bool {
        matches!(self, LazyLayoutKey::User(_))
    }

    /// Returns true if this is a default index-based key.
    #[inline]
    pub fn is_index_key(self) -> bool {
        matches!(self, LazyLayoutKey::Index(_))
    }
}

/// Marker type for lazy scope DSL.
#[doc(hidden)]
pub struct LazyScopeMarker;

/// Receiver scope for lazy list content definition.
///
/// Used by [`LazyColumn`] and [`LazyRow`] to define list items.
/// Matches Jetpack Compose's `LazyListScope`.
///
/// # Example
///
/// ```rust,ignore
/// lazy_column(modifier, state, |scope| {
///     // Single item
///     scope.item(Some(0), None, || {
///         Text::new("Header")
///     });
///
///     // Multiple items
///     scope.items(data.len(), Some(|i| data[i].id), None, |i| {
///         Text::new(data[i].name.clone())
///     });
/// });
/// ```
pub trait LazyListScope {
    /// Adds a single item to the list.
    ///
    /// # Arguments
    /// * `key` - Optional stable key for the item
    /// * `content_type` - Optional content type for efficient reuse
    /// * `content` - Closure that emits the item content
    fn item<F>(&mut self, key: Option<u64>, content_type: Option<u64>, content: F)
    where
        F: Fn() + 'static;

    /// Adds multiple items to the list.
    ///
    /// # Arguments
    /// * `count` - Number of items to add
    /// * `key` - Optional function to generate stable keys from index
    /// * `content_type` - Optional function to generate content types from index
    /// * `item_content` - Closure that emits content for each item
    fn items<K, C, F>(
        &mut self,
        count: usize,
        key: Option<K>,
        content_type: Option<C>,
        item_content: F,
    ) where
        K: Fn(usize) -> u64 + 'static,
        C: Fn(usize) -> u64 + 'static,
        F: Fn(usize) + 'static;
}

/// Internal representation of a lazy list item interval.
///
/// Based on JC's `LazyLayoutIntervalContent.Interval`.
/// Uses Rc for shared ownership of closures (not Clone).
pub struct LazyListInterval {
    /// Start index of this interval in the total item list.
    pub start_index: usize,

    /// Number of items in this interval.
    pub count: usize,

    /// Key generator for items in this interval.
    /// Based on JC's `Interval.key: ((index: Int) -> Any)?`
    pub key: Option<Rc<dyn Fn(usize) -> u64>>,

    /// Content type generator for items in this interval.
    /// Based on JC's `Interval.type: ((index: Int) -> Any?)`
    pub content_type: Option<Rc<dyn Fn(usize) -> u64>>,

    /// Content generator for items in this interval.
    /// Takes the local index within the interval.
    pub content: Rc<dyn Fn(usize)>,
}

impl std::fmt::Debug for LazyListInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LazyListInterval")
            .field("start_index", &self.start_index)
            .field("count", &self.count)
            .finish_non_exhaustive()
    }
}

/// Builder that collects intervals during scope execution.
///
/// Based on JC's `LazyLayoutIntervalContent` with `IntervalList`.
pub struct LazyListIntervalContent {
    intervals: Vec<LazyListInterval>,
    total_count: usize,
    /// Cached slot_id→index mapping for O(1) key lookups.
    /// Built lazily on first key lookup, invalidated when content changes.
    key_cache: RefCell<Option<HashMap<u64, usize>>>,
}

impl LazyListIntervalContent {
    /// Creates a new empty interval content.
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
            total_count: 0,
            key_cache: RefCell::new(None),
        }
    }

    /// Invalidates the key cache. Called when content is modified.
    fn invalidate_cache(&self) {
        *self.key_cache.borrow_mut() = None;
    }

    /// Builds the key→index cache for O(1) lookups.
    /// Only builds cache for lists with <= 10000 items to avoid memory issues.
    fn ensure_cache(&self) {
        const MAX_CACHE_SIZE: usize = 10000;

        let mut cache = self.key_cache.borrow_mut();
        if cache.is_some() {
            return; // Already built
        }

        if self.total_count > MAX_CACHE_SIZE {
            return; // Too large to cache
        }

        let mut map = HashMap::with_capacity(self.total_count);
        for index in 0..self.total_count {
            let slot_id = self.get_key(index).to_slot_id();
            map.insert(slot_id, index);
        }
        *cache = Some(map);
    }

    /// Returns the total number of items across all intervals.
    /// Matches JC's `LazyLayoutIntervalContent.itemCount`.
    pub fn item_count(&self) -> usize {
        self.total_count
    }

    /// Returns the intervals.
    pub fn intervals(&self) -> &[LazyListInterval] {
        &self.intervals
    }

    /// Gets the key for an item at the given global index.
    ///
    /// Returns a [`LazyLayoutKey`] that distinguishes between user-provided keys
    /// and default index-based keys to prevent collisions.
    ///
    /// Matches JC's `LazyLayoutIntervalContent.getKey(index)` pattern.
    pub fn get_key(&self, index: usize) -> LazyLayoutKey {
        if let Some((interval, local_index)) = self.find_interval(index) {
            if let Some(key_fn) = &interval.key {
                return LazyLayoutKey::User(key_fn(local_index));
            }
        }
        // Default key wraps the index (matches JC's getDefaultLazyLayoutKey)
        LazyLayoutKey::Index(index)
    }

    /// Gets the content type for an item at the given global index.
    /// Matches JC's `LazyLayoutIntervalContent.getContentType(index)`.
    pub fn get_content_type(&self, index: usize) -> Option<u64> {
        if let Some((interval, local_index)) = self.find_interval(index) {
            if let Some(type_fn) = &interval.content_type {
                return Some(type_fn(local_index));
            }
        }
        None
    }

    /// Invokes the content closure for an item at the given global index.
    ///
    /// Matches JC's `withInterval` pattern where block is called with
    /// local index and interval content.
    pub fn invoke_content(&self, index: usize) {
        if let Some((interval, local_index)) = self.find_interval(index) {
            (interval.content)(local_index);
        }
    }

    /// Executes a block with the interval containing the given global index.
    /// Matches JC's `withInterval(globalIndex, block)`.
    pub fn with_interval<T, F>(&self, global_index: usize, block: F) -> Option<T>
    where
        F: FnOnce(usize, &LazyListInterval) -> T,
    {
        self.find_interval(global_index)
            .map(|(interval, local_index)| block(local_index, interval))
    }

    /// Returns the index of an item with the given key, or None if not found.
    /// Matches JC's `LazyLayoutItemProvider.getIndex(key: Any): Int`.
    ///
    /// This is used for scroll position stability - when items are added/removed,
    /// the scroll position can be maintained by finding the new index of the
    /// item that was previously at the scroll position (identified by key).
    ///
    /// Uses cached HashMap for O(1) lookup when the list has <= 10000 items.
    /// For larger lists, use [`get_index_by_key_in_range`] with a [`NearestRangeState`].
    #[must_use]
    pub fn get_index_by_key(&self, key: LazyLayoutKey) -> Option<usize> {
        // Convert key to slot_id and use the cache
        let slot_id = key.to_slot_id();
        self.get_index_by_slot_id(slot_id)
    }

    /// Returns the index of an item with the given key, searching only within the range.
    /// Used with NearestRangeState for O(1) key lookup in large lists.
    pub fn get_index_by_key_in_range(
        &self,
        key: LazyLayoutKey,
        range: std::ops::Range<usize>,
    ) -> Option<usize> {
        let start = range.start.min(self.total_count);
        let end = range.end.min(self.total_count);
        (start..end).find(|&index| self.get_key(index) == key)
    }

    /// Returns the index of an item with the given slot ID, or None if not found.
    ///
    /// This is used for scroll position stability when the stored key is a slot ID (u64).
    /// Slot IDs are generated by `LazyLayoutKey::to_slot_id()`.
    ///
    /// Uses cached HashMap for O(1) lookup when the list has <= 10000 items.
    /// For larger lists, use [`get_index_by_slot_id_in_range`] with a range.
    #[must_use]
    pub fn get_index_by_slot_id(&self, slot_id: u64) -> Option<usize> {
        // Try to use cache first (O(1) lookup)
        self.ensure_cache();
        if let Some(cache) = self.key_cache.borrow().as_ref() {
            return cache.get(&slot_id).copied();
        }

        // Cache wasn't built (list too large), fall back to ranged search
        // This shouldn't happen in practice since callers should use
        // get_index_by_slot_id_in_range for large lists
        log::debug!(
            "get_index_by_slot_id: no cache for large list ({} items), returning None",
            self.total_count
        );
        None
    }

    /// Returns the index of an item with the given slot ID, searching only within the range.
    pub fn get_index_by_slot_id_in_range(
        &self,
        slot_id: u64,
        range: std::ops::Range<usize>,
    ) -> Option<usize> {
        let start = range.start.min(self.total_count);
        let end = range.end.min(self.total_count);
        (start..end).find(|&index| self.get_key(index).to_slot_id() == slot_id)
    }

    /// Finds the interval containing the given global index.
    /// Returns the interval and the local index within it.
    /// P2 FIX: Uses binary search for O(log n) instead of linear O(n).
    fn find_interval(&self, index: usize) -> Option<(&LazyListInterval, usize)> {
        if self.intervals.is_empty() || index >= self.total_count {
            return None;
        }

        // Binary search to find the interval containing this index
        let pos = self
            .intervals
            .partition_point(|interval| interval.start_index + interval.count <= index);

        if pos < self.intervals.len() {
            let interval = &self.intervals[pos];
            if index >= interval.start_index && index < interval.start_index + interval.count {
                let local_index = index - interval.start_index;
                return Some((interval, local_index));
            }
        }
        None
    }
}

impl Default for LazyListIntervalContent {
    fn default() -> Self {
        Self::new()
    }
}

impl LazyListScope for LazyListIntervalContent {
    fn item<F>(&mut self, key: Option<u64>, content_type: Option<u64>, content: F)
    where
        F: Fn() + 'static,
    {
        self.invalidate_cache(); // Content is changing
        let start_index = self.total_count;
        self.intervals.push(LazyListInterval {
            start_index,
            count: 1,
            key: key.map(|k| Rc::new(move |_| k) as Rc<dyn Fn(usize) -> u64>),
            content_type: content_type.map(|t| Rc::new(move |_| t) as Rc<dyn Fn(usize) -> u64>),
            content: Rc::new(move |_| content()),
        });
        self.total_count += 1;
    }

    fn items<K, C, F>(
        &mut self,
        count: usize,
        key: Option<K>,
        content_type: Option<C>,
        item_content: F,
    ) where
        K: Fn(usize) -> u64 + 'static,
        C: Fn(usize) -> u64 + 'static,
        F: Fn(usize) + 'static,
    {
        if count == 0 {
            return;
        }

        self.invalidate_cache(); // Content is changing
        let start_index = self.total_count;
        self.intervals.push(LazyListInterval {
            start_index,
            count,
            key: key.map(|k| Rc::new(k) as Rc<dyn Fn(usize) -> u64>),
            content_type: content_type.map(|c| Rc::new(c) as Rc<dyn Fn(usize) -> u64>),
            content: Rc::new(item_content),
        });
        self.total_count += count;
    }
}

/// Extension trait for adding convenience methods to [`LazyListScope`].
///
/// These methods provide ergonomic APIs for common use cases. They use `Rc<[T]>`
/// internally to avoid deep-copying the entire data slice into the closure.
pub trait LazyListScopeExt: LazyListScope {
    /// Adds items from a slice with an item-aware content closure.
    ///
    /// Uses shared ownership (`Rc<[T]>`) to avoid deep-copying the data.
    /// The closure captures a reference-counted pointer, not a full copy.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = vec!["Apple", "Banana", "Cherry"];
    /// scope.items_slice(&data, |item| {
    ///     Text(item.to_string(), Modifier::empty());
    /// });
    /// ```
    fn items_slice<T, F>(&mut self, items: &[T], item_content: F)
    where
        T: Clone + 'static,
        F: Fn(&T) + 'static,
    {
        // Use Rc<[T]> for O(1) closure capture instead of O(n) deep copy.
        // The Rc clone inside the closure is just a pointer copy + refcount increment.
        let items_rc: Rc<[T]> = items.to_vec().into();
        self.items(
            items.len(),
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_rc.get(index) {
                    item_content(item);
                }
            },
        );
    }

    /// Adds indexed items from a slice.
    ///
    /// Uses shared ownership (`Rc<[T]>`) to avoid deep-copying the data.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = vec!["Apple", "Banana", "Cherry"];
    /// scope.items_indexed(&data, |index, item| {
    ///     Text(format!("{}. {}", index + 1, item), Modifier::empty());
    /// });
    /// ```
    fn items_indexed<T, F>(&mut self, items: &[T], item_content: F)
    where
        T: Clone + 'static,
        F: Fn(usize, &T) + 'static,
    {
        // Use Rc<[T]> for O(1) closure capture instead of O(n) deep copy.
        let items_rc: Rc<[T]> = items.to_vec().into();
        self.items(
            items.len(),
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_rc.get(index) {
                    item_content(index, item);
                }
            },
        );
    }
}

impl<T: LazyListScope + ?Sized> LazyListScopeExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn test_single_item() {
        let mut content = LazyListIntervalContent::new();
        let called = Rc::new(Cell::new(false));
        let called_clone = Rc::clone(&called);

        content.item(Some(42), None, move || {
            called_clone.set(true);
        });

        assert_eq!(content.item_count(), 1);
        assert_eq!(content.get_key(0), LazyLayoutKey::User(42));

        content.invoke_content(0);
        assert!(called.get());
    }

    #[test]
    fn test_multiple_items() {
        let mut content = LazyListIntervalContent::new();

        content.items(
            5,
            Some(|i| (i * 10) as u64),
            None::<fn(usize) -> u64>,
            |_i| {},
        );

        assert_eq!(content.item_count(), 5);
        assert_eq!(content.get_key(0), LazyLayoutKey::User(0));
        assert_eq!(content.get_key(1), LazyLayoutKey::User(10));
        assert_eq!(content.get_key(4), LazyLayoutKey::User(40));
    }

    #[test]
    fn test_mixed_intervals() {
        let mut content = LazyListIntervalContent::new();

        // Header
        content.item(Some(100), None, || {});

        // Items
        content.items(3, Some(|i| i as u64), None::<fn(usize) -> u64>, |_| {});

        // Footer
        content.item(Some(200), None, || {});

        assert_eq!(content.item_count(), 5);
        assert_eq!(content.get_key(0), LazyLayoutKey::User(100)); // Header
        assert_eq!(content.get_key(1), LazyLayoutKey::User(0)); // First item
        assert_eq!(content.get_key(2), LazyLayoutKey::User(1)); // Second item
        assert_eq!(content.get_key(3), LazyLayoutKey::User(2)); // Third item
        assert_eq!(content.get_key(4), LazyLayoutKey::User(200)); // Footer
    }

    #[test]
    fn test_with_interval() {
        let mut content = LazyListIntervalContent::new();
        content.items(
            5,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            |_| {},
        );

        let result = content.with_interval(3, |local_idx, interval| (local_idx, interval.count));

        assert_eq!(result, Some((3, 5)));
    }

    #[test]
    fn test_user_keys_dont_collide_with_default_keys() {
        let mut content = LazyListIntervalContent::new();

        // Item 0: User key = 0
        content.item(Some(0), None, || {});
        // Item 1: No key (default Index(1))
        content.item(None, None, || {});
        // Item 2: User key = 1
        content.item(Some(1), None, || {});

        // User key 0 should NOT equal default Index(0)
        assert_eq!(content.get_key(0), LazyLayoutKey::User(0));
        assert_eq!(content.get_key(1), LazyLayoutKey::Index(1));
        assert_eq!(content.get_key(2), LazyLayoutKey::User(1));

        // Critically: User(0) != Index(1) and User(1) != Index(1)
        assert_ne!(content.get_key(0), content.get_key(1));
        assert_ne!(content.get_key(2), content.get_key(1));

        // Keys should convert to different slot IDs
        assert_ne!(
            content.get_key(0).to_slot_id(),
            content.get_key(1).to_slot_id()
        );
    }

    #[test]
    fn test_slot_id_collision_prevention() {
        // User(0) and Index(0) should produce different slot IDs
        let user_key = LazyLayoutKey::User(0);
        let index_key = LazyLayoutKey::Index(0);

        assert_ne!(user_key.to_slot_id(), index_key.to_slot_id());

        // Index keys should have high bit set
        assert!(index_key.to_slot_id() & (1u64 << 63) != 0);
        // User keys should NOT have high bit set (assuming normal usage)
        assert!(user_key.to_slot_id() & (1u64 << 63) == 0);
    }
}
