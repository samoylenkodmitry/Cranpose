//! DSL scope for building lazy list content.
//!
//! Provides [`LazyListScope`] trait and implementation for the ergonomic
//! `item {}` / `items {}` API used in `LazyColumn` and `LazyRow`.
//!
//! Based on JC's `LazyLayoutIntervalContent` pattern.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

static USER_OVERFLOW_LOGGED: AtomicBool = AtomicBool::new(false);
static INDEX_OVERFLOW_LOGGED: AtomicBool = AtomicBool::new(false);

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
    /// Tag for user-provided keys: high 2 bits = 00
    const USER_TAG: u64 = 0b00 << 62;
    /// Tag for index-based keys: high 2 bits = 01
    const INDEX_TAG: u64 = 0b01 << 62;
    /// Mask for the value portion (bits 0-61).
    /// Based on u64 output type, so this is platform-independent.
    const VALUE_MASK: u64 = (1u64 << 62) - 1;

    /// Converts to u64 for slot ID usage with guaranteed non-overlapping ranges.
    ///
    /// # Encoding
    /// Uses high 2 bits of the 64-bit slot ID as a type tag:
    /// - User keys: `0b00` tag + 62-bit value (range: 0x0000... - 0x3FFF...)
    /// - Index keys: `0b01` tag + 62-bit value (range: 0x4000... - 0x7FFF...)
    ///
    /// # ⚠️ Large Key Handling
    /// Values larger than 62 bits are **mixed down to 62 bits**. This avoids panics
    /// for extreme indices (e.g. `usize::MAX`) but introduces a small chance of
    /// collisions for out-of-range keys. Prefer keys that fit in 62 bits when
    /// you need guaranteed collision-free IDs.
    ///
    /// # Cross-Platform Safety
    /// The slot ID is always `u64` regardless of target platform.
    #[inline]
    pub fn to_slot_id(self) -> u64 {
        match self {
            // NOTE: Values beyond 62 bits are mixed to preserve stability.
            LazyLayoutKey::User(k) => {
                let value = Self::normalize_value(k, "User", &USER_OVERFLOW_LOGGED);
                Self::USER_TAG | value
            }
            LazyLayoutKey::Index(i) => {
                let value = Self::normalize_value(i as u64, "Index", &INDEX_OVERFLOW_LOGGED);
                Self::INDEX_TAG | value
            }
        }
    }

    #[inline]
    fn normalize_value(value: u64, kind: &'static str, logged: &AtomicBool) -> u64 {
        if value <= Self::VALUE_MASK {
            value
        } else {
            if !logged.swap(true, Ordering::Relaxed) {
                log::warn!(
                    "LazyList {} key {:#018x} exceeds 62 bits; mixing to 62 bits to avoid overflow",
                    kind,
                    value
                );
            }
            Self::mix_to_value_bits(value)
        }
    }

    #[inline]
    fn mix_to_value_bits(mut value: u64) -> u64 {
        value ^= value >> 33;
        value = value.wrapping_mul(0xff51afd7ed558ccd);
        value ^= value >> 33;
        value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
        value ^= value >> 33;
        value & Self::VALUE_MASK
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
    ///
    /// This cache is always built regardless of list size to guarantee O(1) performance.
    /// Memory usage is approximately 16 bytes per item (slot_id: u64 + index: usize).
    /// For a list of 100,000 items, this is ~1.6MB of cache memory.
    fn ensure_cache(&self) {
        let mut cache = self.key_cache.borrow_mut();
        if cache.is_some() {
            return; // Already built
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

    /// Threshold below which linear search is faster than building a HashMap cache.
    const CACHE_THRESHOLD: usize = 64;

    /// Returns the index of an item with the given slot ID, or None if not found.
    ///
    /// This is used for scroll position stability when the stored key is a slot ID (u64).
    /// Slot IDs are generated by `LazyLayoutKey::to_slot_id()`.
    ///
    /// Uses cached HashMap for O(1) lookup on large lists. For small lists (< 64 items),
    /// uses linear search to avoid HashMap allocation overhead.
    /// For hot paths during scrolling, prefer [`get_index_by_slot_id_in_range`] first.
    #[must_use]
    pub fn get_index_by_slot_id(&self, slot_id: u64) -> Option<usize> {
        // For small lists, linear search is faster than building/using the cache
        if self.total_count <= Self::CACHE_THRESHOLD {
            return (0..self.total_count)
                .find(|&index| self.get_key(index).to_slot_id() == slot_id);
        }

        // Try to use cache first (O(1) lookup)
        self.ensure_cache();
        if let Some(cache) = self.key_cache.borrow().as_ref() {
            return cache.get(&slot_id).copied();
        }

        // Safety fallback: should not happen since ensure_cache always builds the map.
        log::warn!(
            "get_index_by_slot_id: cache unexpectedly missing ({} items), using linear search",
            self.total_count
        );
        (0..self.total_count).find(|&index| self.get_key(index).to_slot_id() == slot_id)
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

use crate::lazy::item_provider::LazyLayoutItemProvider;

/// Implements [`LazyLayoutItemProvider`] to formalize the item factory contract.
/// This provides the same functionality as the existing methods but through
/// the standardized trait interface.
impl LazyLayoutItemProvider for LazyListIntervalContent {
    fn item_count(&self) -> usize {
        self.total_count
    }

    fn get_key(&self, index: usize) -> u64 {
        // Delegate to the existing get_key and convert to slot_id
        LazyListIntervalContent::get_key(self, index).to_slot_id()
    }

    fn get_content_type(&self, index: usize) -> Option<u64> {
        // Delegate to the inherent method which returns Option<u64>
        LazyListIntervalContent::get_content_type(self, index)
    }

    fn get_index(&self, key: u64) -> Option<usize> {
        // Use the cached lookup
        self.get_index_by_slot_id(key)
    }
}

/// Extension trait for adding convenience methods to [`LazyListScope`].
///
/// Provides ergonomic APIs for common use cases with different performance tradeoffs:
///
/// | Method | Upfront Cost | Use Case |
/// |--------|--------------|----------|
/// | [`items_slice`] | O(n) copy | Convenience, small data |
/// | [`items_slice_rc`] | O(1) | Data already in `Rc<[T]>` |
/// | [`items_with_provider`] | O(1) | Lazy on-demand access |
pub trait LazyListScopeExt: LazyListScope {
    /// Adds items from a slice with an item-aware content closure.
    ///
    /// # ⚠️ Performance Warning
    ///
    /// **This method performs an O(n) allocation and copy of the entire slice upfront.**
    ///
    /// This copy is required to satisfy Rust's `'static` closure requirements for
    /// the lazy list item factory. For small lists (< 1000 items) this is typically
    /// acceptable, but for large datasets consider these alternatives:
    ///
    /// | Alternative | When to Use |
    /// |-------------|-------------|
    /// | [`items_slice_rc`] | Data is already in `Rc<[T]>` - **zero copy** |
    /// | [`items_vec`] | Data is in a `Vec<T>` you can give up ownership of - **efficient** |
    /// | [`items_with_provider`] | Need lazy on-demand access - **zero copy** |
    ///
    /// After the initial copy, the closure captures a reference-counted pointer,
    /// so subsequent Rc clones are O(1).
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
        // Note: to_vec() is O(n) allocation + copy. This is documented above.
        // For zero-copy, use items_slice_rc() or items_with_provider().
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

    /// Adds items from a `Vec<T>`, taking ownership.
    ///
    /// **Efficient ownership transfer**: Uses `Rc::from(vec)` which avoids copying
    /// elements if the allocation fits (or does a simple realloc).
    /// Use this when you have a `Vec` and want to pass it to the list.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = vec!["Apple".to_string(), "Banana".to_string()];
    /// scope.items_vec(data, |item| {
    ///     Text(item.to_string(), Modifier::empty());
    /// });
    /// ```
    fn items_vec<T, F>(&mut self, items: Vec<T>, item_content: F)
    where
        T: 'static,
        F: Fn(&T) + 'static,
    {
        let len = items.len();
        let items_rc: Rc<[T]> = Rc::from(items);
        self.items(
            len,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_rc.get(index) {
                    item_content(item);
                }
            },
        );
    }

    /// Adds indexed items from a collection (Slice, Vec, or Rc).
    ///
    /// This method is generic over the input type `L` which must be convertible to `Rc<[T]>`.
    /// This allows for efficient ownership transfer (zero-copy for `Vec` and `Rc`) or
    /// convenient usage with slices (which will perform a copy).
    ///
    /// # Performance Note
    ///
    /// - **`Vec<T>`**: Zero-copy (ownership transfer). Efficient.
    /// - **`Rc<[T]>`**: Zero-copy (ownership transfer). Efficient.
    /// - **`&[T]`**: **O(N) copy**. Convenient for small lists, but avoid for large datasets.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Efficient Vec usage (zero-copy)
    /// let data = vec!["Apple".to_string(), "Banana".to_string()];
    /// scope.items_indexed(data, |index, item| { ... });
    ///
    /// // Slice usage (performs copy)
    /// let data_slice = &["Apple", "Banana"];
    /// scope.items_indexed(data_slice, |index, item| { ... });
    /// ```
    fn items_indexed<T, L, F>(&mut self, items: L, item_content: F)
    where
        T: 'static,
        L: Into<Rc<[T]>>,
        F: Fn(usize, &T) + 'static,
    {
        let items_rc: Rc<[T]> = items.into();
        self.items(
            items_rc.len(),
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items_rc.get(index) {
                    item_content(index, item);
                }
            },
        );
    }

    /// Adds items from a pre-existing `Rc<[T]>` without cloning.
    ///
    /// **Zero-copy optimization**: If you already have your data in an `Rc<[T]>`,
    /// use this method to avoid the O(n) clone that `items_slice` performs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
    /// scope.items_slice_rc(Rc::clone(&data), |item| {
    ///     Text(item.to_string(), Modifier::empty());
    /// });
    /// ```
    fn items_slice_rc<T, F>(&mut self, items: Rc<[T]>, item_content: F)
    where
        T: 'static,
        F: Fn(&T) + 'static,
    {
        let len = items.len();
        self.items(
            len,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items.get(index) {
                    item_content(item);
                }
            },
        );
    }

    /// Adds indexed items from a pre-existing `Rc<[T]>` without cloning.
    ///
    /// **Zero-copy optimization**: If you already have your data in an `Rc<[T]>`,
    /// use this method to avoid the O(n) clone that `items_indexed` performs.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
    /// scope.items_indexed_rc(Rc::clone(&data), |index, item| {
    ///     Text(format!("{}. {}", index + 1, item), Modifier::empty());
    /// });
    /// ```
    fn items_indexed_rc<T, F>(&mut self, items: Rc<[T]>, item_content: F)
    where
        T: 'static,
        F: Fn(usize, &T) + 'static,
    {
        let len = items.len();
        self.items(
            len,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = items.get(index) {
                    item_content(index, item);
                }
            },
        );
    }

    /// Adds items using a provider function for on-demand data access.
    ///
    /// **Zero-allocation pattern**: Instead of storing data, the provider function
    /// is called lazily when each item is rendered. This avoids any upfront
    /// allocation or cloning.
    ///
    /// The provider should return `Some(T)` for valid indices and `None` for
    /// out-of-bounds access. The item is passed by value to the content closure.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = vec!["Apple", "Banana", "Cherry"];
    /// scope.items_with_provider(
    ///     data.len(),
    ///     move |index| data.get(index).copied(),
    ///     |item| {
    ///         Text(item.to_string(), Modifier::empty());
    ///     },
    /// );
    /// ```
    fn items_with_provider<T, P, F>(&mut self, count: usize, provider: P, item_content: F)
    where
        T: 'static,
        P: Fn(usize) -> Option<T> + 'static,
        F: Fn(T) + 'static,
    {
        self.items(
            count,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = provider(index) {
                    item_content(item);
                }
            },
        );
    }

    /// Adds indexed items using a provider function for on-demand data access.
    ///
    /// **Zero-allocation pattern**: Instead of storing data, the provider function
    /// is called lazily when each item is rendered. This avoids any upfront
    /// allocation or cloning.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = vec!["Apple", "Banana", "Cherry"];
    /// scope.items_indexed_with_provider(
    ///     data.len(),
    ///     move |index| data.get(index).copied(),
    ///     |index, item| {
    ///         Text(format!("{}. {}", index + 1, item), Modifier::empty());
    ///     },
    /// );
    /// ```
    fn items_indexed_with_provider<T, P, F>(&mut self, count: usize, provider: P, item_content: F)
    where
        T: 'static,
        P: Fn(usize) -> Option<T> + 'static,
        F: Fn(usize, T) + 'static,
    {
        self.items(
            count,
            None::<fn(usize) -> u64>,
            None::<fn(usize) -> u64>,
            move |index| {
                if let Some(item) = provider(index) {
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

        // User keys have tag 0b00 in high 2 bits (bits 62-63)
        // Index keys have tag 0b01 in high 2 bits (bit 62 set)
        assert_eq!(user_key.to_slot_id(), 0); // 0b00 << 62 | 0 = 0
        assert_eq!(index_key.to_slot_id(), 1u64 << 62); // 0b01 << 62 | 0

        // User keys occupy range 0x0000... to 0x3FFF...
        // Index keys occupy range 0x4000... to 0x7FFF...
        assert!(user_key.to_slot_id() < (1u64 << 62));
        assert!(index_key.to_slot_id() >= (1u64 << 62));
        assert!(index_key.to_slot_id() < (2u64 << 62));

        // Any user value within 62 bits maps to the user range
        let user_max = LazyLayoutKey::User((1u64 << 62) - 1);
        assert!(
            user_max.to_slot_id() < (1u64 << 62),
            "User keys stay in user range"
        );
        assert_eq!(user_max.to_slot_id(), (1u64 << 62) - 1); // All 62 value bits set

        // Any index value within 62 bits maps to the index range
        let index_large = LazyLayoutKey::Index(((1u64 << 62) - 1) as usize);
        assert!(
            index_large.to_slot_id() >= (1u64 << 62),
            "Index keys stay in index range"
        );
        assert!(
            index_large.to_slot_id() < (2u64 << 62),
            "Index keys below reserved range"
        );

        // See release-only test for the documented high-bit collision behavior.
    }

    #[test]
    fn test_user_key_overflow_is_stable_and_tagged() {
        let user_max = LazyLayoutKey::User(u64::MAX);
        let slot = user_max.to_slot_id();
        assert_eq!(slot, user_max.to_slot_id());
        assert!(slot < (1u64 << 62));
    }

    #[test]
    fn test_index_key_overflow_is_stable_and_tagged() {
        let index_max = LazyLayoutKey::Index(usize::MAX);
        let slot = index_max.to_slot_id();
        assert_eq!(slot, index_max.to_slot_id());
        assert!(slot >= (1u64 << 62));
        assert!(slot < (2u64 << 62));
    }

    #[test]
    fn test_user_key_high_bits_influence_slot_id() {
        let key_low = LazyLayoutKey::User(0x0000_0000_0000_0001);
        let key_high = LazyLayoutKey::User(0x4000_0000_0000_0001); // Differs in bit 62
        assert_ne!(
            key_low.to_slot_id(),
            key_high.to_slot_id(),
            "High bits are mixed into the slot id to avoid truncation collisions"
        );
    }

    // ============================================================
    // LazyListScopeExt tests
    // ============================================================

    #[test]
    fn test_items_slice() {
        let mut content = LazyListIntervalContent::new();
        let data = vec!["Apple", "Banana", "Cherry"];
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_slice(&data, move |item: &&str| {
            items_clone.borrow_mut().push((*item).to_string());
        });

        assert_eq!(content.item_count(), 3);

        // Invoke each item and check the callback received correct values
        for i in 0..3 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(*visited, vec!["Apple", "Banana", "Cherry"]);
    }

    #[test]
    fn test_items_indexed() {
        let mut content = LazyListIntervalContent::new();
        // Use Vec -> Into<Rc<[T]>> directly (efficient)
        let data = vec!["Apple".to_string(), "Banana".to_string(), "Cherry".to_string()];
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_indexed(data, move |index, item: &String| {
            items_clone.borrow_mut().push((index, item.clone()));
        });

        assert_eq!(content.item_count(), 3);

        for i in 0..3 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(
            *visited,
            vec![
                (0, "Apple".to_string()),
                (1, "Banana".to_string()),
                (2, "Cherry".to_string())
            ]
        );
    }

    #[test]
    fn test_items_indexed_slice() {
        let mut content = LazyListIntervalContent::new();
        // Use Slice -> Into<Rc<[T]>> (performs copy)
        let data = vec!["Apple", "Banana", "Cherry"];
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        // Note: passing slice explicitly (generic bound doesn't do deref coercion from &Vec)
        content.items_indexed(data.as_slice(), move |index, item: &&str| {
            items_clone.borrow_mut().push((index, (*item).to_string()));
        });

        assert_eq!(content.item_count(), 3);

        for i in 0..3 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(
            *visited,
            vec![
                (0, "Apple".to_string()),
                (1, "Banana".to_string()),
                (2, "Cherry".to_string())
            ]
        );
    }

    #[test]
    fn test_items_slice_rc() {
        let mut content = LazyListIntervalContent::new();
        let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_slice_rc(Rc::clone(&data), move |item: &String| {
            items_clone.borrow_mut().push(item.clone());
        });

        assert_eq!(content.item_count(), 2);

        for i in 0..2 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(*visited, vec!["Apple", "Banana"]);
    }

    #[test]
    fn test_items_indexed_rc() {
        let mut content = LazyListIntervalContent::new();
        let data: Rc<[String]> = Rc::from(vec!["Apple".into(), "Banana".into()]);
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_indexed_rc(Rc::clone(&data), move |index, item: &String| {
            items_clone.borrow_mut().push((index, item.clone()));
        });

        assert_eq!(content.item_count(), 2);

        for i in 0..2 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(
            *visited,
            vec![(0, "Apple".to_string()), (1, "Banana".to_string())]
        );
    }

    #[test]
    fn test_items_with_provider() {
        let mut content = LazyListIntervalContent::new();
        let data = ["Apple", "Banana", "Cherry"];
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_with_provider(
            data.len(),
            move |index| data.get(index).copied(),
            move |item: &str| {
                items_clone.borrow_mut().push(item.to_string());
            },
        );

        assert_eq!(content.item_count(), 3);

        for i in 0..3 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(*visited, vec!["Apple", "Banana", "Cherry"]);
    }

    #[test]
    fn test_items_indexed_with_provider() {
        let mut content = LazyListIntervalContent::new();
        let data = ["Apple", "Banana", "Cherry"];
        let items_visited = Rc::new(RefCell::new(Vec::new()));
        let items_clone = items_visited.clone();

        content.items_indexed_with_provider(
            data.len(),
            move |index| data.get(index).copied(),
            move |index, item: &str| {
                items_clone.borrow_mut().push((index, item.to_string()));
            },
        );

        assert_eq!(content.item_count(), 3);

        for i in 0..3 {
            content.invoke_content(i);
        }

        let visited = items_visited.borrow();
        assert_eq!(
            *visited,
            vec![
                (0, "Apple".to_string()),
                (1, "Banana".to_string()),
                (2, "Cherry".to_string())
            ]
        );
    }

    #[test]
    fn test_large_list_cache_works() {
        // Test that get_index_by_slot_id uses O(1) cache for lists > 10k items
        // (previously this would fall back to O(N) linear search)
        let mut content = LazyListIntervalContent::new();

        // Create a list with 20,000 items (above the old 10k limit)
        content.items(
            20_000,
            Some(|i| (i * 7) as u64), // Unique keys
            None::<fn(usize) -> u64>,
            |_| {},
        );

        // Verify lookup works for item near the end
        let key_19999 = content.get_key(19999);
        assert_eq!(key_19999, LazyLayoutKey::User(19999 * 7));

        // Verify get_index_by_slot_id finds the correct index (should be O(1) now)
        let slot_id = key_19999.to_slot_id();
        let found_index = content.get_index_by_slot_id(slot_id);
        assert_eq!(found_index, Some(19999));

        // Also test a middle item
        let key_10000 = content.get_key(10000);
        let slot_id_mid = key_10000.to_slot_id();
        let found_mid = content.get_index_by_slot_id(slot_id_mid);
        assert_eq!(found_mid, Some(10000));
    }

}
