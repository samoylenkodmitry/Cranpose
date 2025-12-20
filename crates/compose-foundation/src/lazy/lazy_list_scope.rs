//! DSL scope for building lazy list content.
//!
//! Provides [`LazyListScope`] trait and implementation for the ergonomic
//! `item {}` / `items {}` API used in `LazyColumn` and `LazyRow`.
//!
//! Based on JC's `LazyLayoutIntervalContent` pattern.

use std::rc::Rc;

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
}

impl LazyListIntervalContent {
    /// Creates a new empty interval content.
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
            total_count: 0,
        }
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
    /// Matches JC's `LazyLayoutIntervalContent.getKey(index)`.
    pub fn get_key(&self, index: usize) -> u64 {
        if let Some((interval, local_index)) = self.find_interval(index) {
            if let Some(key_fn) = &interval.key {
                return key_fn(local_index);
            }
        }
        // Default key is the index itself (matches JC's getDefaultLazyLayoutKey)
        index as u64
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
    pub fn get_index_by_key(&self, key: u64) -> Option<usize> {
        // For small lists, do full O(n) search
        const SMALL_LIST_THRESHOLD: usize = 1000;
        if self.total_count <= SMALL_LIST_THRESHOLD {
            return (0..self.total_count).find(|&index| self.get_key(index) == key);
        }
        
        // For large lists, return None - caller should use get_index_by_key_in_range
        // with a NearestRangeState to limit the search
        None
    }

    /// Returns the index of an item with the given key, searching only within the range.
    /// Used with NearestRangeState for O(1) key lookup in large lists.
    pub fn get_index_by_key_in_range(&self, key: u64, range: std::ops::Range<usize>) -> Option<usize> {
        let start = range.start.min(self.total_count);
        let end = range.end.min(self.total_count);
        (start..end).find(|&index| self.get_key(index) == key)
    }

    /// Finds the interval containing the given global index.
    /// Returns the interval and the local index within it.
    /// P2 FIX: Uses binary search for O(log n) instead of linear O(n).
    fn find_interval(&self, index: usize) -> Option<(&LazyListInterval, usize)> {
        if self.intervals.is_empty() || index >= self.total_count {
            return None;
        }
        
        // Binary search to find the interval containing this index
        let pos = self.intervals.partition_point(|interval| {
            interval.start_index + interval.count <= index
        });
        
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
pub trait LazyListScopeExt: LazyListScope {
    /// Adds items from a slice with an item-aware content closure.
    fn items_slice<T, F>(&mut self, items: &[T], item_content: F)
    where
        T: Clone + 'static,
        F: Fn(&T) + 'static,
    {
        let items_clone: Vec<T> = items.to_vec();
        self.items(
            items.len(),
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_clone.get(index) {
                    item_content(item);
                }
            },
        );
    }

    /// Adds indexed items from a slice.
    fn items_indexed<T, F>(&mut self, items: &[T], item_content: F)
    where
        T: Clone + 'static,
        F: Fn(usize, &T) + 'static,
    {
        let items_clone: Vec<T> = items.to_vec();
        self.items(
            items.len(),
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_clone.get(index) {
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
        assert_eq!(content.get_key(0), 42);

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
        assert_eq!(content.get_key(0), 0);
        assert_eq!(content.get_key(1), 10);
        assert_eq!(content.get_key(4), 40);
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
        assert_eq!(content.get_key(0), 100); // Header
        assert_eq!(content.get_key(1), 0); // First item
        assert_eq!(content.get_key(2), 1); // Second item
        assert_eq!(content.get_key(3), 2); // Third item
        assert_eq!(content.get_key(4), 200); // Footer
    }

    #[test]
    fn test_with_interval() {
        let mut content = LazyListIntervalContent::new();
        content.items(5, None::<fn(usize) -> u64>, None::<fn(usize) -> u64>, |_| {});

        let result = content.with_interval(3, |local_idx, interval| {
            (local_idx, interval.count)
        });

        assert_eq!(result, Some((3, 5)));
    }
}
