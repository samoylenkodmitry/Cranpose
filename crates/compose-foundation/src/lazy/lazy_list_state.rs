//! Lazy list state management.
//!
//! Provides [`LazyListState`] for controlling and observing lazy list scroll position.
//!
//! Design follows Jetpack Compose's LazyListState/LazyListScrollPosition pattern:
//! - Reactive properties are backed by `MutableState<T>`:
//!   - `first_visible_item_index`, `first_visible_item_scroll_offset`
//!   - `can_scroll_forward`, `can_scroll_backward`
//!   - `stats` (items_in_use, items_in_pool)
//! - Non-reactive internals (caches, callbacks, prefetch, diagnostic counters) are in inner state

use std::cell::RefCell;
use std::rc::Rc;

use compose_core::MutableState;
use compose_macros::composable;

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

// ─────────────────────────────────────────────────────────────────────────────
// LazyListScrollPosition - Reactive scroll position (matches JC design)
// ─────────────────────────────────────────────────────────────────────────────

/// Contains the current scroll position represented by the first visible item
/// index and the first visible item scroll offset.
///
/// This is a Copy type that holds reactive state. Reading `index` or `scroll_offset`
/// during composition creates a snapshot dependency for automatic recomposition.
///
/// Matches Jetpack Compose's `LazyListScrollPosition` design.
#[derive(Clone, Copy)]
pub struct LazyListScrollPosition {
    /// The index of the first visible item (reactive).
    index: MutableState<usize>,
    /// The scroll offset of the first visible item (reactive).
    scroll_offset: MutableState<f32>,
    /// Non-reactive internal state (key tracking, nearest range).
    inner: MutableState<Rc<RefCell<ScrollPositionInner>>>,
}

/// Non-reactive internal state for scroll position.
struct ScrollPositionInner {
    /// The last known key of the item at index position.
    /// Used for scroll position stability across data changes.
    last_known_first_item_key: Option<u64>,
    /// Sliding window range for optimized key lookups.
    nearest_range_state: NearestRangeState,
}

impl LazyListScrollPosition {
    /// Returns the index of the first visible item (reactive read).
    pub fn index(&self) -> usize {
        self.index.get()
    }

    /// Returns the scroll offset of the first visible item (reactive read).
    pub fn scroll_offset(&self) -> f32 {
        self.scroll_offset.get()
    }

    /// Updates the scroll position from a measurement result.
    ///
    /// Called after layout measurement to update the reactive scroll position.
    /// This stores the key for scroll position stability and updates the nearest range.
    pub(crate) fn update_from_measure_result(
        &self,
        first_visible_index: usize,
        first_visible_scroll_offset: f32,
        first_visible_item_key: Option<u64>,
    ) {
        // Update internal state (key tracking, nearest range)
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.last_known_first_item_key = first_visible_item_key;
            inner.nearest_range_state.update(first_visible_index);
        });

        // Only update reactive state if value changed (avoids recomposition loops)
        let old_index = self.index.get();
        if old_index != first_visible_index {
            self.index.set(first_visible_index);
        }
        let old_offset = self.scroll_offset.get();
        if (old_offset - first_visible_scroll_offset).abs() > 0.001 {
            self.scroll_offset.set(first_visible_scroll_offset);
        }
    }

    /// Requests a new position and clears the last known key.
    /// Used for programmatic scrolls (scroll_to_item).
    pub(crate) fn request_position_and_forget_last_known_key(
        &self,
        index: usize,
        scroll_offset: f32,
    ) {
        // Update reactive state
        if self.index.get() != index {
            self.index.set(index);
        }
        if (self.scroll_offset.get() - scroll_offset).abs() > 0.001 {
            self.scroll_offset.set(scroll_offset);
        }
        // Clear key and update nearest range
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.last_known_first_item_key = None;
            inner.nearest_range_state.update(index);
        });
    }

    /// Adjusts scroll position if the first visible item was moved.
    /// Returns the adjusted index.
    pub(crate) fn update_if_first_item_moved<F>(
        &self,
        new_item_count: usize,
        find_by_key: F,
    ) -> usize
    where
        F: Fn(u64) -> Option<usize>,
    {
        let current_index = self.index.get();
        let last_key = self.inner.with(|rc| rc.borrow().last_known_first_item_key);

        let new_index = match last_key {
            None => current_index.min(new_item_count.saturating_sub(1)),
            Some(key) => find_by_key(key)
                .unwrap_or_else(|| current_index.min(new_item_count.saturating_sub(1))),
        };

        if current_index != new_index {
            self.index.set(new_index);
            self.inner.with(|rc| {
                rc.borrow_mut().nearest_range_state.update(new_index);
            });
        }
        new_index
    }

    /// Returns the nearest range for optimized key lookups.
    pub fn nearest_range(&self) -> std::ops::Range<usize> {
        self.inner
            .with(|rc| rc.borrow().nearest_range_state.range())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LazyListState - Main state object
// ─────────────────────────────────────────────────────────────────────────────

/// State object for lazy list scroll position tracking.
///
/// Holds the current scroll position and provides methods to programmatically
/// control scrolling. Create with [`remember_lazy_list_state()`] in composition.
///
/// This type is `Copy`, so it can be passed to multiple closures without explicit `.clone()` calls.
///
/// # Reactive Properties (read during composition triggers recomposition)
/// - `first_visible_item_index()` - index of first visible item
/// - `first_visible_item_scroll_offset()` - scroll offset within first item
/// - `can_scroll_forward()` - whether more items exist below/right
/// - `can_scroll_backward()` - whether more items exist above/left
/// - `stats()` - lifecycle statistics (`items_in_use`, `items_in_pool`)
///
/// # Non-Reactive Properties
/// - `stats().total_composed` - total items composed (diagnostic)
/// - `stats().reuse_count` - items reused from pool (diagnostic)
/// - `layout_info()` - detailed layout information
///
/// # Example
///
/// ```rust,ignore
/// let state = remember_lazy_list_state();
///
/// // Scroll to item 50
/// state.scroll_to_item(50, 0.0);
///
/// // Get current visible item (reactive read)
/// println!("First visible: {}", state.first_visible_item_index());
/// ```
#[derive(Clone, Copy)]
pub struct LazyListState {
    /// Scroll position with reactive index and offset (matches JC design).
    scroll_position: LazyListScrollPosition,
    /// Whether we can scroll forward (reactive, matches JC).
    can_scroll_forward_state: MutableState<bool>,
    /// Whether we can scroll backward (reactive, matches JC).
    can_scroll_backward_state: MutableState<bool>,
    /// Reactive stats state for triggering recomposition when stats change.
    /// Only contains items_in_use and items_in_pool (diagnostic counters are in inner).
    stats_state: MutableState<LazyLayoutStats>,
    /// Non-reactive internal state (caches, callbacks, prefetch, layout info).
    inner: MutableState<Rc<RefCell<LazyListStateInner>>>,
}

// Implement PartialEq by comparing inner pointers for identity.
// This allows LazyListState to be used as a composable function parameter.
impl PartialEq for LazyListState {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.inner_ptr(), other.inner_ptr())
    }
}

/// Non-reactive internal state for LazyListState.
struct LazyListStateInner {
    /// Scroll delta to be consumed in the next layout pass.
    scroll_to_be_consumed: f32,

    /// Pending scroll-to-item request.
    pending_scroll_to_index: Option<(usize, f32)>,

    /// Layout info from the last measure pass.
    layout_info: LazyListLayoutInfo,

    /// Invalidation callbacks.
    invalidate_callbacks: Vec<(u64, Rc<dyn Fn()>)>,
    next_callback_id: u64,

    /// Whether a layout invalidation callback has been registered for this state.
    /// Used to prevent duplicate registrations on recomposition.
    has_layout_invalidation_callback: bool,

    /// Diagnostic counters (non-reactive - not typically displayed in UI).
    total_composed: usize,
    reuse_count: usize,

    /// Cache of recently measured item sizes (index -> main_axis_size).
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
}

/// Creates a remembered [`LazyListState`] with default initial position.
///
/// This is the recommended way to create a `LazyListState` in composition.
/// The returned state is `Copy` and can be passed to multiple closures without `.clone()`.
///
/// # Example
///
/// ```rust,ignore
/// let list_state = remember_lazy_list_state();
///
/// // Pass to multiple closures - no .clone() needed!
/// LazyColumn(modifier, list_state, spec, content);
/// Button(move || list_state.scroll_to_item(0, 0.0));
/// ```
#[composable]
pub fn remember_lazy_list_state() -> LazyListState {
    remember_lazy_list_state_with_position(0, 0.0)
}

/// Creates a remembered [`LazyListState`] with the specified initial position.
///
/// The returned state is `Copy` and can be passed to multiple closures without `.clone()`.
#[composable]
pub fn remember_lazy_list_state_with_position(
    initial_first_visible_item_index: usize,
    initial_first_visible_item_scroll_offset: f32,
) -> LazyListState {
    // Create scroll position with reactive fields (matches JC LazyListScrollPosition)
    let scroll_position = LazyListScrollPosition {
        index: compose_core::useState(|| initial_first_visible_item_index),
        scroll_offset: compose_core::useState(|| initial_first_visible_item_scroll_offset),
        inner: compose_core::useState(|| {
            Rc::new(RefCell::new(ScrollPositionInner {
                last_known_first_item_key: None,
                nearest_range_state: NearestRangeState::new(initial_first_visible_item_index),
            }))
        }),
    };

    // Non-reactive internal state
    let inner = compose_core::useState(|| {
        Rc::new(RefCell::new(LazyListStateInner {
            scroll_to_be_consumed: 0.0,
            pending_scroll_to_index: None,
            layout_info: LazyListLayoutInfo::default(),
            invalidate_callbacks: Vec::new(),
            next_callback_id: 1,
            has_layout_invalidation_callback: false,
            total_composed: 0,
            reuse_count: 0,
            item_size_cache: std::collections::HashMap::new(),
            item_size_lru: std::collections::VecDeque::new(),
            average_item_size: super::DEFAULT_ITEM_SIZE_ESTIMATE,
            total_measured_items: 0,
            prefetch_scheduler: PrefetchScheduler::new(),
            prefetch_strategy: PrefetchStrategy::default(),
            last_scroll_direction: 0.0,
        }))
    });

    // Reactive state
    let can_scroll_forward_state = compose_core::useState(|| false);
    let can_scroll_backward_state = compose_core::useState(|| false);
    let stats_state = compose_core::useState(LazyLayoutStats::default);

    LazyListState {
        scroll_position,
        can_scroll_forward_state,
        can_scroll_backward_state,
        stats_state,
        inner,
    }
}

impl LazyListState {
    /// Returns a pointer to the inner state for unique identification.
    /// Used by scroll gesture detection to create unique keys.
    pub fn inner_ptr(&self) -> *const () {
        self.inner.with(|rc| Rc::as_ptr(rc) as *const ())
    }

    /// Returns the index of the first visible item.
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes to the index will trigger recomposition.
    pub fn first_visible_item_index(&self) -> usize {
        // Delegate to scroll_position (reactive read)
        self.scroll_position.index()
    }

    /// Returns the scroll offset of the first visible item.
    ///
    /// This is the amount the first item is scrolled off-screen (positive = scrolled up/left).
    /// When called during composition, this creates a reactive subscription
    /// so that changes to the offset will trigger recomposition.
    pub fn first_visible_item_scroll_offset(&self) -> f32 {
        // Delegate to scroll_position (reactive read)
        self.scroll_position.scroll_offset()
    }

    /// Returns the layout info from the last measure pass.
    pub fn layout_info(&self) -> LazyListLayoutInfo {
        self.inner.with(|rc| rc.borrow().layout_info.clone())
    }

    /// Returns the current item lifecycle statistics.
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes to `items_in_use` or `items_in_pool` will trigger recomposition.
    /// The `total_composed` and `reuse_count` fields are diagnostic and non-reactive.
    pub fn stats(&self) -> LazyLayoutStats {
        // Read reactive state (creates subscription) and combine with non-reactive counters
        let reactive = self.stats_state.get();
        let (total_composed, reuse_count) = self.inner.with(|rc| {
            let inner = rc.borrow();
            (inner.total_composed, inner.reuse_count)
        });
        LazyLayoutStats {
            items_in_use: reactive.items_in_use,
            items_in_pool: reactive.items_in_pool,
            total_composed,
            reuse_count,
        }
    }

    /// Updates the item lifecycle statistics.
    ///
    /// Called by the layout measurement after updating slot pools.
    /// Triggers recomposition if `items_in_use` or `items_in_pool` changed.
    pub fn update_stats(&self, items_in_use: usize, items_in_pool: usize) {
        let current = self.stats_state.get();

        // Hysteresis: only trigger reactive update when items_in_use INCREASES
        // or DECREASES by more than 1. This prevents the 5→4→5→4 oscillation
        // that happens at boundary conditions during slow upward scroll.
        //
        // Rationale:
        // - Items becoming visible (increase): user should see count update immediately
        // - Items going off-screen by 1: minor fluctuation, wait for significant change
        // - Items going off-screen by 2+: significant change, update immediately
        let should_update_reactive = if items_in_use > current.items_in_use {
            // Increase: always update (new items visible)
            true
        } else if items_in_use < current.items_in_use {
            // Decrease: only update if by more than 1 (prevents oscillation)
            current.items_in_use - items_in_use > 1
        } else {
            false
        };

        if should_update_reactive {
            self.stats_state.set(LazyLayoutStats {
                items_in_use,
                items_in_pool,
                ..current
            });
        }
        // Note: pool-only changes are intentionally not committed to reactive state
        // to prevent the 5→4→5 oscillation that caused slow upward scroll hang.
    }

    /// Records that an item was composed (either new or reused).
    ///
    /// This updates diagnostic counters in non-reactive state.
    /// Does NOT trigger recomposition.
    pub fn record_composition(&self, was_reused: bool) {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.total_composed += 1;
            if was_reused {
                inner.reuse_count += 1;
            }
        });
    }

    /// Records the scroll direction for prefetch calculations.
    /// Positive = scrolling forward (content moving up), negative = backward.
    pub fn record_scroll_direction(&self, delta: f32) {
        if delta.abs() > 0.001 {
            self.inner.with(|rc| {
                rc.borrow_mut().last_scroll_direction = delta.signum();
            });
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
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            let direction = inner.last_scroll_direction;
            let strategy = inner.prefetch_strategy.clone();
            inner.prefetch_scheduler.update(
                first_visible_index,
                last_visible_index,
                total_items,
                direction,
                &strategy,
            );
        });
    }

    /// Returns the indices that should be prefetched.
    /// Consumes the prefetch queue.
    pub fn take_prefetch_indices(&self) -> Vec<usize> {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            let mut indices = Vec::new();
            while let Some(idx) = inner.prefetch_scheduler.next_prefetch() {
                indices.push(idx);
            }
            indices
        })
    }

    /// Scrolls to the specified item index.
    ///
    /// # Arguments
    /// * `index` - The index of the item to scroll to
    /// * `scroll_offset` - Additional offset within the item (default 0)
    pub fn scroll_to_item(&self, index: usize, scroll_offset: f32) {
        // Store pending scroll request
        self.inner.with(|rc| {
            rc.borrow_mut().pending_scroll_to_index = Some((index, scroll_offset));
        });

        // Delegate to scroll_position which handles reactive updates and key clearing
        self.scroll_position
            .request_position_and_forget_last_known_key(index, scroll_offset);

        self.invalidate();
    }

    /// Dispatches a raw scroll delta.
    ///
    /// Returns the amount of scroll actually consumed.
    ///
    /// This triggers layout invalidation via registered callbacks. The callbacks are
    /// registered by LazyColumnImpl/LazyRowImpl with schedule_layout_repass(node_id),
    /// which provides O(subtree) performance instead of O(entire app).
    pub fn dispatch_scroll_delta(&self, delta: f32) -> f32 {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.scroll_to_be_consumed += delta;
        });
        self.invalidate();
        delta // Will be adjusted during layout
    }

    /// Consumes and returns the pending scroll delta.
    ///
    /// Called by the layout during measure.
    pub(crate) fn consume_scroll_delta(&self) -> f32 {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            let delta = inner.scroll_to_be_consumed;
            inner.scroll_to_be_consumed = 0.0;
            delta
        })
    }

    /// Peeks at the pending scroll delta without consuming it.
    ///
    /// Used for direction inference before measurement consumes the delta.
    /// This is more accurate than comparing first visible index, especially for:
    /// - Scrolling within the same item (partial scroll)
    /// - Variable height items where scroll offset changes without index change
    pub fn peek_scroll_delta(&self) -> f32 {
        self.inner.with(|rc| rc.borrow().scroll_to_be_consumed)
    }

    /// Consumes and returns the pending scroll-to-item request.
    ///
    /// Called by the layout during measure.
    pub(crate) fn consume_scroll_to_index(&self) -> Option<(usize, f32)> {
        self.inner
            .with(|rc| rc.borrow_mut().pending_scroll_to_index.take())
    }

    /// Caches the measured size of an item for scroll estimation.
    ///
    /// Uses a HashMap + VecDeque LRU pattern with O(1) insertion and eviction.
    /// Re-measurement of existing items (uncommon during normal scrolling)
    /// requires O(n) VecDeque position lookup, but the cache is small (100 items).
    ///
    /// # Performance Note
    /// If profiling shows this as a bottleneck, consider using the `lru` crate
    /// for O(1) update-in-place operations, or a linked hash map.
    pub fn cache_item_size(&self, index: usize, size: f32) {
        use std::collections::hash_map::Entry;
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
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
        });
    }

    /// Gets a cached item size if available.
    pub fn get_cached_size(&self, index: usize) -> Option<f32> {
        self.inner
            .with(|rc| rc.borrow().item_size_cache.get(&index).copied())
    }

    /// Returns the running average of measured item sizes.
    pub fn average_item_size(&self) -> f32 {
        self.inner.with(|rc| rc.borrow().average_item_size)
    }

    /// Returns the current nearest range for optimized key lookup.
    pub fn nearest_range(&self) -> std::ops::Range<usize> {
        // Delegate to scroll_position
        self.scroll_position.nearest_range()
    }

    /// Updates the scroll position from a layout pass.
    ///
    /// Called by the layout after measurement.
    pub(crate) fn update_scroll_position(
        &self,
        first_visible_item_index: usize,
        first_visible_item_scroll_offset: f32,
    ) {
        self.scroll_position.update_from_measure_result(
            first_visible_item_index,
            first_visible_item_scroll_offset,
            None,
        );
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
        self.scroll_position.update_from_measure_result(
            first_visible_item_index,
            first_visible_item_scroll_offset,
            Some(first_visible_item_key),
        );
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
        // Delegate to scroll_position
        self.scroll_position
            .update_if_first_item_moved(new_item_count, get_index_by_key)
    }

    /// Updates the layout info from a layout pass.
    pub(crate) fn update_layout_info(&self, info: LazyListLayoutInfo) {
        self.inner.with(|rc| rc.borrow_mut().layout_info = info);
    }

    /// Returns whether we can scroll forward (more items below/right).
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes will trigger recomposition.
    pub fn can_scroll_forward(&self) -> bool {
        self.can_scroll_forward_state.get()
    }

    /// Returns whether we can scroll backward (more items above/left).
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes will trigger recomposition.
    pub fn can_scroll_backward(&self) -> bool {
        self.can_scroll_backward_state.get()
    }

    /// Updates the scroll bounds after layout measurement.
    ///
    /// Called by the layout after measurement to update can_scroll_forward/backward.
    pub(crate) fn update_scroll_bounds(&self) {
        // Compute can_scroll_forward from layout info
        let can_forward = self.inner.with(|rc| {
            let inner = rc.borrow();
            let info = &inner.layout_info;
            // Use effective viewport end (accounting for after_content_padding)
            // Without this, lists with padding can report false while still scrollable
            let viewport_end = info.viewport_size - info.after_content_padding;
            if let Some(last_visible) = info.visible_items_info.last() {
                last_visible.index < info.total_items_count.saturating_sub(1)
                    || (last_visible.offset + last_visible.size) > viewport_end
            } else {
                false
            }
        });

        // Compute can_scroll_backward from scroll position
        let can_backward =
            self.scroll_position.index() > 0 || self.scroll_position.scroll_offset() > 0.0;

        // Update reactive state only if changed
        if self.can_scroll_forward_state.get() != can_forward {
            self.can_scroll_forward_state.set(can_forward);
        }
        if self.can_scroll_backward_state.get() != can_backward {
            self.can_scroll_backward_state.set(can_backward);
        }
    }

    /// Adds an invalidation callback.
    pub fn add_invalidate_callback(&self, callback: Rc<dyn Fn()>) -> u64 {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            let id = inner.next_callback_id;
            inner.next_callback_id += 1;
            inner.invalidate_callbacks.push((id, callback));
            id
        })
    }

    /// Tries to register a layout invalidation callback.
    ///
    /// Returns true if the callback was registered, false if one was already registered.
    /// This prevents duplicate registrations on recomposition.
    pub fn try_register_layout_callback(&self, callback: Rc<dyn Fn()>) -> bool {
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            if inner.has_layout_invalidation_callback {
                return false;
            }
            inner.has_layout_invalidation_callback = true;
            let id = inner.next_callback_id;
            inner.next_callback_id += 1;
            inner.invalidate_callbacks.push((id, callback));
            true
        })
    }

    /// Removes an invalidation callback.
    pub fn remove_invalidate_callback(&self, id: u64) {
        self.inner.with(|rc| {
            rc.borrow_mut()
                .invalidate_callbacks
                .retain(|(cb_id, _)| *cb_id != id);
        });
    }

    fn invalidate(&self) {
        // Clone callbacks to avoid holding the borrow while calling them
        // This prevents re-entrancy issues if a callback triggers another state update
        let callbacks: Vec<_> = self.inner.with(|rc| {
            rc.borrow()
                .invalidate_callbacks
                .iter()
                .map(|(_, cb)| Rc::clone(cb))
                .collect()
        });

        for callback in callbacks {
            callback();
        }
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

/// Test helpers for creating LazyListState without composition context.
#[cfg(test)]
pub mod test_helpers {
    use super::*;
    use compose_core::{DefaultScheduler, Runtime};
    use std::sync::Arc;

    /// Creates a test runtime and keeps it alive for the duration of the closure.
    /// Use this to create LazyListState in unit tests.
    pub fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    /// Creates a new LazyListState for testing.
    /// Must be called within `with_test_runtime`.
    pub fn new_lazy_list_state() -> LazyListState {
        new_lazy_list_state_with_position(0, 0.0)
    }

    /// Creates a new LazyListState for testing with initial position.
    /// Must be called within `with_test_runtime`.
    pub fn new_lazy_list_state_with_position(
        initial_first_visible_item_index: usize,
        initial_first_visible_item_scroll_offset: f32,
    ) -> LazyListState {
        // Create scroll position with reactive fields (matches JC LazyListScrollPosition)
        let scroll_position = LazyListScrollPosition {
            index: compose_core::mutableStateOf(initial_first_visible_item_index),
            scroll_offset: compose_core::mutableStateOf(initial_first_visible_item_scroll_offset),
            inner: compose_core::mutableStateOf(Rc::new(RefCell::new(ScrollPositionInner {
                last_known_first_item_key: None,
                nearest_range_state: NearestRangeState::new(initial_first_visible_item_index),
            }))),
        };

        // Non-reactive internal state
        let inner = compose_core::mutableStateOf(Rc::new(RefCell::new(LazyListStateInner {
            scroll_to_be_consumed: 0.0,
            pending_scroll_to_index: None,
            layout_info: LazyListLayoutInfo::default(),
            invalidate_callbacks: Vec::new(),
            next_callback_id: 1,
            has_layout_invalidation_callback: false,
            total_composed: 0,
            reuse_count: 0,
            item_size_cache: std::collections::HashMap::new(),
            item_size_lru: std::collections::VecDeque::new(),
            average_item_size: super::super::DEFAULT_ITEM_SIZE_ESTIMATE,
            total_measured_items: 0,
            prefetch_scheduler: PrefetchScheduler::new(),
            prefetch_strategy: PrefetchStrategy::default(),
            last_scroll_direction: 0.0,
        })));

        // Reactive state
        let can_scroll_forward_state = compose_core::mutableStateOf(false);
        let can_scroll_backward_state = compose_core::mutableStateOf(false);
        let stats_state = compose_core::mutableStateOf(LazyLayoutStats::default());

        LazyListState {
            scroll_position,
            can_scroll_forward_state,
            can_scroll_backward_state,
            stats_state,
            inner,
        }
    }
}
