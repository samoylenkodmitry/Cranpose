//! Lazy list state management.
//!
//! Provides [`LazyListState`] for controlling and observing lazy list scroll position.

use std::cell::RefCell;

use super::nearest_range::NearestRangeState;
use super::prefetch::{PrefetchScheduler, PrefetchStrategy};

/// Statistics about lazy layout item lifecycle.
///
/// Used for testing and debugging virtualization behavior.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LazyLayoutStats {
    /// Number of items currently composed and visible.
    pub items_in_use: usize,

    /// Number of items in the recycle pool (available for reuse).
    pub items_in_pool: usize,

    /// Total number of items that have been composed.
    pub total_composed: usize,

    /// Number of items that were reused instead of newly composed.
    pub reuse_count: usize,
}

/// State object for lazy list scroll position tracking.
///
/// Holds the current scroll position and provides methods to programmatically
/// control scrolling. Create with [`LazyListState::new`] or use
/// `remember_lazy_list_state()` in composition.
///
/// # Example
///
/// ```rust,ignore
/// let state = LazyListState::new();
///
/// // Scroll to item 50
/// state.scroll_to_item(50, 0.0);
///
/// // Get current visible item
/// println!("First visible: {}", state.first_visible_item_index());
/// ```
#[derive(Clone)]
pub struct LazyListState {
    inner: std::rc::Rc<RefCell<LazyListStateInner>>,
    /// Reactive stats state for triggering recomposition when stats change.
    /// Lazily initialized on first read during composition.
    stats_state: std::rc::Rc<RefCell<Option<compose_core::MutableState<LazyLayoutStats>>>>,
}

struct LazyListStateInner {
    /// Index of the first visible item.
    first_visible_item_index: usize,

    /// Scroll offset within the first visible item (in pixels).
    first_visible_item_scroll_offset: f32,

    /// Key of the first visible item (for scroll position stability).
    /// Used to find the item after data changes.
    last_known_first_visible_key: Option<u64>,

    /// Scroll delta to be consumed in the next layout pass.
    scroll_to_be_consumed: f32,

    /// Pending scroll-to-item request.
    pending_scroll_to_index: Option<(usize, f32)>,

    /// Layout info from the last measure pass.
    layout_info: LazyListLayoutInfo,

    /// Invalidation callbacks.
    invalidate_callbacks: Vec<(u64, Box<dyn Fn()>)>,
    next_callback_id: u64,

    /// Item lifecycle statistics.
    stats: LazyLayoutStats,

    /// Flag indicating stats changed since last check (for deferred UI update)
    stats_changed: bool,

    /// Cache of recently measured item sizes (index -> main_axis_size).
    /// Limited capacity with LRU eviction for O(1) performance.
    item_size_cache: std::collections::HashMap<usize, f32>,
    /// LRU order tracking - front is oldest, back is newest.
    item_size_lru: std::collections::VecDeque<usize>,

    /// Running average of measured item sizes for estimation.
    average_item_size: f32,
    total_measured_items: usize,

    /// Prefetch scheduler for pre-composing items.
    prefetch_scheduler: PrefetchScheduler,

    /// Prefetch strategy configuration.
    prefetch_strategy: PrefetchStrategy,

    /// Last scroll delta direction for prefetch.
    last_scroll_direction: f32,

    /// Sliding window range for optimized key lookups.
    nearest_range_state: NearestRangeState,
}

impl LazyListState {
    /// Creates a new [`LazyListState`] with default initial position.
    pub fn new() -> Self {
        Self::with_initial_position(0, 0.0)
    }

    /// Creates a new [`LazyListState`] with the specified initial position.
    pub fn with_initial_position(
        initial_first_visible_item_index: usize,
        initial_first_visible_item_scroll_offset: f32,
    ) -> Self {
        Self {
            inner: std::rc::Rc::new(RefCell::new(LazyListStateInner {
                first_visible_item_index: initial_first_visible_item_index,
                first_visible_item_scroll_offset: initial_first_visible_item_scroll_offset,
                last_known_first_visible_key: None,
                scroll_to_be_consumed: 0.0,
                pending_scroll_to_index: None,
                layout_info: LazyListLayoutInfo::default(),
                invalidate_callbacks: Vec::new(),
                next_callback_id: 1,
                stats: LazyLayoutStats::default(),
                stats_changed: false,
                item_size_cache: std::collections::HashMap::new(),
                item_size_lru: std::collections::VecDeque::new(),
                average_item_size: super::DEFAULT_ITEM_SIZE_ESTIMATE,
                total_measured_items: 0,
                prefetch_scheduler: PrefetchScheduler::new(),
                prefetch_strategy: PrefetchStrategy::default(),
                last_scroll_direction: 0.0,
                nearest_range_state: NearestRangeState::new(initial_first_visible_item_index),
            })),
            stats_state: std::rc::Rc::new(RefCell::new(None)),
        }
    }

    /// Returns a pointer to the inner state for unique identification.
    /// Used by scroll gesture detection to create unique keys.
    pub fn inner_ptr(&self) -> *const () {
        std::rc::Rc::as_ptr(&self.inner) as *const ()
    }

    /// Returns the index of the first visible item.
    pub fn first_visible_item_index(&self) -> usize {
        self.inner.borrow().first_visible_item_index
    }

    /// Returns the scroll offset of the first visible item.
    ///
    /// This is the amount the first item is scrolled off-screen (positive = scrolled up/left).
    pub fn first_visible_item_scroll_offset(&self) -> f32 {
        self.inner.borrow().first_visible_item_scroll_offset
    }

    /// Returns the layout info from the last measure pass.
    pub fn layout_info(&self) -> LazyListLayoutInfo {
        self.inner.borrow().layout_info.clone()
    }

    /// Returns the current item lifecycle statistics.
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes to stats will trigger recomposition.
    pub fn stats(&self) -> LazyLayoutStats {
        // Try to use reactive state if available
        let mut stats_state_ref = self.stats_state.borrow_mut();
        if let Some(reactive_state) = stats_state_ref.as_ref() {
            // Read from reactive state - this creates a subscription during composition
            return reactive_state.get();
        }

        // Try to lazily initialize reactive state if runtime is available
        if let Some(state) = self.try_init_stats_state() {
            let value = state.get();
            *stats_state_ref = Some(state);
            return value;
        }

        // Fallback to non-reactive read (before runtime is initialized)
        self.inner.borrow().stats.clone()
    }

    /// Try to initialize the reactive stats state.
    /// Returns Some if runtime is available, None otherwise.
    fn try_init_stats_state(&self) -> Option<compose_core::MutableState<LazyLayoutStats>> {
        // try_mutableStateOf returns None if no runtime is available yet
        compose_core::try_mutableStateOf(self.inner.borrow().stats.clone())
    }

    /// Updates the item lifecycle statistics.
    ///
    /// Called by the layout measurement after updating slot pools.
    /// Triggers recomposition if reactive state is initialized.
    pub fn update_stats(&self, items_in_use: usize, items_in_pool: usize) {
        // Update the inner stats
        let new_stats = {
            let mut inner = self.inner.borrow_mut();
            if inner.stats.items_in_use != items_in_use
                || inner.stats.items_in_pool != items_in_pool
            {
                inner.stats.items_in_use = items_in_use;
                inner.stats.items_in_pool = items_in_pool;
                inner.stats_changed = true;
                Some(inner.stats.clone())
            } else {
                None
            }
        };

        // Update reactive state if initialized (triggers recomposition)
        if let Some(new_stats) = new_stats {
            if let Some(reactive_state) = self.stats_state.borrow().as_ref() {
                reactive_state.set(new_stats);
            }
        }
    }

    /// Checks if stats changed since last check and clears the flag.
    /// Returns true if stats changed, false otherwise.
    /// Use this to trigger recomposition after measurement.
    pub fn check_stats_changed(&self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let changed = inner.stats_changed;
        inner.stats_changed = false;
        changed
    }

    /// Records that an item was composed (either new or reused).
    pub fn record_composition(&self, was_reused: bool) {
        let mut inner = self.inner.borrow_mut();
        inner.stats.total_composed += 1;
        if was_reused {
            inner.stats.reuse_count += 1;
        }
        // Note: We don't update reactive state here because total_composed
        // and reuse_count are typically not displayed in the UI.
    }

    /// Records the scroll direction for prefetch calculations.
    /// Positive = scrolling forward (content moving up), negative = backward.
    pub fn record_scroll_direction(&self, delta: f32) {
        if delta.abs() > 0.001 {
            self.inner.borrow_mut().last_scroll_direction = delta.signum();
        }
    }

    /// Updates the prefetch queue based on current visible items.
    /// Should be called after measurement to queue items for pre-composition.
    pub fn update_prefetch_queue(
        &self,
        first_visible_index: usize,
        last_visible_index: usize,
        total_items: usize,
    ) {
        let mut inner = self.inner.borrow_mut();
        let direction = inner.last_scroll_direction;
        let strategy = inner.prefetch_strategy.clone();
        inner.prefetch_scheduler.update(
            first_visible_index,
            last_visible_index,
            total_items,
            direction,
            &strategy,
        );
    }

    /// Returns the indices that should be prefetched.
    /// Consumes the prefetch queue.
    pub fn take_prefetch_indices(&self) -> Vec<usize> {
        let mut inner = self.inner.borrow_mut();
        let mut indices = Vec::new();
        while let Some(idx) = inner.prefetch_scheduler.next_prefetch() {
            indices.push(idx);
        }
        indices
    }

    /// Scrolls to the specified item index.
    ///
    /// # Arguments
    /// * `index` - The index of the item to scroll to
    /// * `scroll_offset` - Additional offset within the item (default 0)
    pub fn scroll_to_item(&self, index: usize, scroll_offset: f32) {
        let mut inner = self.inner.borrow_mut();
        inner.pending_scroll_to_index = Some((index, scroll_offset));
        // Also update the first visible index immediately so that if a second measure
        // happens before the next frame, it uses the correct position
        inner.first_visible_item_index = index;
        inner.first_visible_item_scroll_offset = scroll_offset;
        // Clear the last known key to prevent update_scroll_position_if_item_moved
        // from resetting to the old position based on key lookup
        inner.last_known_first_visible_key = None;
        drop(inner);
        self.invalidate();
    }

    /// Dispatches a raw scroll delta.
    ///
    /// Returns the amount of scroll actually consumed.
    pub fn dispatch_scroll_delta(&self, delta: f32) -> f32 {
        let mut inner = self.inner.borrow_mut();
        inner.scroll_to_be_consumed += delta;
        drop(inner);
        self.invalidate();
        delta // Will be adjusted during layout
    }

    /// Consumes and returns the pending scroll delta.
    ///
    /// Called by the layout during measure.
    pub(crate) fn consume_scroll_delta(&self) -> f32 {
        let mut inner = self.inner.borrow_mut();
        let delta = inner.scroll_to_be_consumed;
        inner.scroll_to_be_consumed = 0.0;
        delta
    }

    /// Consumes and returns the pending scroll-to-item request.
    ///
    /// Called by the layout during measure.
    pub(crate) fn consume_scroll_to_index(&self) -> Option<(usize, f32)> {
        self.inner.borrow_mut().pending_scroll_to_index.take()
    }

    /// Caches the measured size of an item for scroll estimation.
    /// Uses LRU eviction for O(1) performance.
    pub fn cache_item_size(&self, index: usize, size: f32) {
        use std::collections::hash_map::Entry;
        let mut inner = self.inner.borrow_mut();
        const MAX_CACHE_SIZE: usize = 100;

        // Check if already in cache (update existing)
        if let Entry::Occupied(mut entry) = inner.item_size_cache.entry(index) {
            // Update value and move to back of LRU
            entry.insert(size);
            // Remove old position from LRU (O(n) but rare - only on re-measurement)
            if let Some(pos) = inner.item_size_lru.iter().position(|&k| k == index) {
                inner.item_size_lru.remove(pos);
            }
            inner.item_size_lru.push_back(index);
            return;
        }

        // Evict oldest entries until under limit - O(1) per eviction
        while inner.item_size_cache.len() >= MAX_CACHE_SIZE {
            if let Some(oldest) = inner.item_size_lru.pop_front() {
                // Only remove if still in cache (may have been updated)
                if inner.item_size_cache.remove(&oldest).is_some() {
                    break; // Removed one entry, now under limit
                }
            } else {
                break; // LRU empty, shouldn't happen
            }
        }

        // Add new entry
        inner.item_size_cache.insert(index, size);
        inner.item_size_lru.push_back(index);

        // Update running average
        inner.total_measured_items += 1;
        let n = inner.total_measured_items as f32;
        inner.average_item_size = inner.average_item_size * ((n - 1.0) / n) + size / n;
    }

    /// Gets a cached item size if available.
    pub fn get_cached_size(&self, index: usize) -> Option<f32> {
        self.inner.borrow().item_size_cache.get(&index).copied()
    }

    /// Returns the running average of measured item sizes.
    pub fn average_item_size(&self) -> f32 {
        self.inner.borrow().average_item_size
    }

    /// Returns the current nearest range for optimized key lookup.
    pub fn nearest_range(&self) -> std::ops::Range<usize> {
        self.inner.borrow().nearest_range_state.range()
    }

    /// Updates the nearest range state based on current scroll position.
    pub fn update_nearest_range(&self) {
        let idx = self.first_visible_item_index();
        self.inner.borrow_mut().nearest_range_state.update(idx);
    }

    /// Updates the scroll position from a layout pass.
    ///
    /// Called by the layout after measurement.
    pub(crate) fn update_scroll_position(
        &self,
        first_visible_item_index: usize,
        first_visible_item_scroll_offset: f32,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.first_visible_item_index = first_visible_item_index;
        inner.first_visible_item_scroll_offset = first_visible_item_scroll_offset;
    }

    /// Updates the scroll position and stores the key of the first visible item.
    ///
    /// Called by the layout after measurement to enable scroll position stability.
    pub(crate) fn update_scroll_position_with_key(
        &self,
        first_visible_item_index: usize,
        first_visible_item_scroll_offset: f32,
        first_visible_item_key: u64,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.first_visible_item_index = first_visible_item_index;
        inner.first_visible_item_scroll_offset = first_visible_item_scroll_offset;
        inner.last_known_first_visible_key = Some(first_visible_item_key);
    }

    /// Adjusts scroll position if the first visible item was moved due to data changes.
    ///
    /// Matches JC's `updateScrollPositionIfTheFirstItemWasMoved`.
    /// If items were inserted/removed before the current scroll position,
    /// this finds the item by its key and updates the index accordingly.
    ///
    /// Returns the adjusted first visible item index.
    pub fn update_scroll_position_if_item_moved<F>(
        &self,
        new_item_count: usize,
        get_index_by_key: F,
    ) -> usize
    where
        F: Fn(u64) -> Option<usize>,
    {
        let mut inner = self.inner.borrow_mut();

        // If no key stored, just clamp index to valid range
        let Some(last_key) = inner.last_known_first_visible_key else {
            inner.first_visible_item_index = inner
                .first_visible_item_index
                .min(new_item_count.saturating_sub(1));
            return inner.first_visible_item_index;
        };

        // Try to find the item by key
        if let Some(new_index) = get_index_by_key(last_key) {
            if new_index != inner.first_visible_item_index {
                // Item moved - update index to maintain scroll position
                inner.first_visible_item_index = new_index;
            }
        } else {
            // Item removed - clamp to valid range
            inner.first_visible_item_index = inner
                .first_visible_item_index
                .min(new_item_count.saturating_sub(1));
        }

        inner.first_visible_item_index
    }

    /// Updates the layout info from a layout pass.
    pub(crate) fn update_layout_info(&self, info: LazyListLayoutInfo) {
        self.inner.borrow_mut().layout_info = info;
    }

    /// Returns whether we can scroll forward (more items below/right).
    pub fn can_scroll_forward(&self) -> bool {
        let inner = self.inner.borrow();
        let info = &inner.layout_info;
        if let Some(last_visible) = info.visible_items_info.last() {
            last_visible.index < info.total_items_count.saturating_sub(1)
                || (last_visible.offset + last_visible.size) > info.viewport_size
        } else {
            false
        }
    }

    /// Returns whether we can scroll backward (more items above/left).
    pub fn can_scroll_backward(&self) -> bool {
        let inner = self.inner.borrow();
        inner.first_visible_item_index > 0 || inner.first_visible_item_scroll_offset > 0.0
    }

    /// Adds an invalidation callback.
    pub fn add_invalidate_callback(&self, callback: Box<dyn Fn()>) -> u64 {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_callback_id;
        inner.next_callback_id += 1;
        inner.invalidate_callbacks.push((id, callback));
        id
    }

    /// Removes an invalidation callback.
    pub fn remove_invalidate_callback(&self, id: u64) {
        let mut inner = self.inner.borrow_mut();
        inner.invalidate_callbacks.retain(|(cb_id, _)| *cb_id != id);
    }

    fn invalidate(&self) {
        let inner = self.inner.borrow();
        for (_, callback) in &inner.invalidate_callbacks {
            callback();
        }
    }
}

impl Default for LazyListState {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about the currently visible items in a lazy list.
#[derive(Clone, Default, Debug)]
pub struct LazyListLayoutInfo {
    /// Information about each visible item.
    pub visible_items_info: Vec<LazyListItemInfo>,

    /// Total number of items in the list.
    pub total_items_count: usize,

    /// Size of the viewport in the main axis.
    pub viewport_size: f32,

    /// Start offset of the viewport (content padding before).
    pub viewport_start_offset: f32,

    /// End offset of the viewport (content padding after).
    pub viewport_end_offset: f32,

    /// Content padding before the first item.
    pub before_content_padding: f32,

    /// Content padding after the last item.
    pub after_content_padding: f32,
}

/// Information about a single visible item in a lazy list.
#[derive(Clone, Debug)]
pub struct LazyListItemInfo {
    /// Index of the item in the data source.
    pub index: usize,

    /// Key of the item.
    pub key: u64,

    /// Offset of the item from the start of the list content.
    pub offset: f32,

    /// Size of the item in the main axis.
    pub size: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let state = LazyListState::new();
        assert_eq!(state.first_visible_item_index(), 0);
        assert_eq!(state.first_visible_item_scroll_offset(), 0.0);
    }

    #[test]
    fn test_scroll_to_item() {
        let state = LazyListState::new();
        state.scroll_to_item(10, 5.0);

        let pending = state.consume_scroll_to_index();
        assert_eq!(pending, Some((10, 5.0)));

        // Should be consumed
        assert_eq!(state.consume_scroll_to_index(), None);
    }

    #[test]
    fn test_scroll_delta() {
        let state = LazyListState::new();
        state.dispatch_scroll_delta(100.0);
        state.dispatch_scroll_delta(50.0);

        let consumed = state.consume_scroll_delta();
        assert_eq!(consumed, 150.0);

        // Should be consumed
        assert_eq!(state.consume_scroll_delta(), 0.0);
    }

    #[test]
    fn test_can_scroll() {
        let state = LazyListState::new();

        // Empty list can't scroll
        assert!(!state.can_scroll_forward());
        assert!(!state.can_scroll_backward());

        // Update with some items
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: vec![
                LazyListItemInfo {
                    index: 0,
                    key: 0,
                    offset: 0.0,
                    size: 50.0,
                },
                LazyListItemInfo {
                    index: 1,
                    key: 1,
                    offset: 50.0,
                    size: 50.0,
                },
            ],
            total_items_count: 10,
            viewport_size: 100.0,
            ..Default::default()
        });

        assert!(state.can_scroll_forward()); // More items after index 1
        assert!(!state.can_scroll_backward()); // At the start
    }
}
