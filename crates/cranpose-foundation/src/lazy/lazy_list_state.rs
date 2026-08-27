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

use std::{cell::RefCell, cmp::Reverse, collections::BinaryHeap, rc::Rc};

use cranpose_core::{MutableState, NodeId, StateId};
use cranpose_macros::composable;

use super::{
    diagnostics,
    nearest_range::NearestRangeState,
    prefetch::{PrefetchScheduler, PrefetchStrategy},
};

const MAX_PENDING_SCROLL_DELTA: f32 = 2000.0;
const ITEM_SIZE_CACHE_CAPACITY: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LazyListMeasureStateSnapshot {
    pub(crate) first_visible_item_index: usize,
    pub(crate) first_visible_item_scroll_offset: f32,
    pub(crate) pending_scroll_delta: f32,
    pub(crate) pending_scroll_to: Option<(usize, f32)>,
    pub(crate) average_item_size: f32,
}

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
/// This is a `Copy` type that holds reactive state. Reading `index` or `scroll_offset`
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
    /// Authoritative first visible item index used by layout and non-reactive reads.
    current_index: usize,
    /// Authoritative first visible item offset used by layout and non-reactive reads.
    current_scroll_offset: f32,
    /// The last known key of the item at index position.
    /// Used for scroll position stability across data changes.
    last_known_first_item_key: Option<u64>,
    /// Sliding window range for optimized key lookups.
    nearest_range_state: NearestRangeState,
}

impl LazyListScrollPosition {
    fn is_alive(&self) -> bool {
        self.index.is_alive() && self.scroll_offset.is_alive() && self.inner.is_alive()
    }

    fn current_index(&self) -> usize {
        self.inner
            .try_with(|rc| rc.borrow().current_index)
            .unwrap_or(0)
    }

    fn current_scroll_offset(&self) -> f32 {
        self.inner
            .try_with(|rc| rc.borrow().current_scroll_offset)
            .unwrap_or(0.0)
    }

    /// Returns the index of the first visible item (reactive read).
    pub fn index(&self) -> usize {
        if !self.index.is_alive() || !self.inner.is_alive() {
            return 0;
        }
        self.index.subscribe_current_scope_only();
        self.current_index()
    }

    /// Returns the scroll offset of the first visible item (reactive read).
    pub fn scroll_offset(&self) -> f32 {
        if !self.scroll_offset.is_alive() || !self.inner.is_alive() {
            return 0.0;
        }
        self.scroll_offset.subscribe_current_scope_only();
        self.current_scroll_offset()
    }

    /// Updates the retained scroll position from a measurement result.
    pub(crate) fn update_from_measure_result(
        &self,
        first_visible_index: usize,
        first_visible_scroll_offset: f32,
        first_visible_item_key: Option<u64>,
    ) {
        if !self.is_alive() {
            return;
        }
        // Update internal state (key tracking, nearest range)
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.current_index = first_visible_index;
            inner.current_scroll_offset = first_visible_scroll_offset;
            inner.last_known_first_item_key = first_visible_item_key;
            inner.nearest_range_state.update(first_visible_index);
        });

        if self.index.get_non_reactive() != first_visible_index {
            self.index.set(first_visible_index);
        }
        if (self.scroll_offset.get_non_reactive() - first_visible_scroll_offset).abs() > 0.001 {
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
        if !self.is_alive() {
            return;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.current_index = index;
            inner.current_scroll_offset = scroll_offset;
            inner.last_known_first_item_key = None;
            inner.nearest_range_state.update(index);
        });

        if self.index.get_non_reactive() != index {
            self.index.set(index);
        }
        if (self.scroll_offset.get_non_reactive() - scroll_offset).abs() > 0.001 {
            self.scroll_offset.set(scroll_offset);
        }
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
        if !self.index.is_alive() || !self.inner.is_alive() {
            return 0;
        }

        let current_index = self.current_index();
        let last_key = self
            .inner
            .try_with(|rc| rc.borrow().last_known_first_item_key)
            .flatten();

        let new_index = match last_key {
            None => current_index.min(new_item_count.saturating_sub(1)),
            Some(key) => find_by_key(key)
                .unwrap_or_else(|| current_index.min(new_item_count.saturating_sub(1))),
        };

        if current_index != new_index {
            self.inner.with(|rc| {
                let mut inner = rc.borrow_mut();
                inner.current_index = new_index;
                inner.nearest_range_state.update(new_index);
            });
            self.index.set(new_index);
        }
        new_index
    }

    /// Returns the nearest range for optimized key lookups.
    pub fn nearest_range(&self) -> std::ops::Range<usize> {
        self.inner
            .try_with(|rc| rc.borrow().nearest_range_state.range())
            .unwrap_or(0..0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LazyListState - Main state object
// ─────────────────────────────────────────────────────────────────────────────

/// State object for lazy list scroll position tracking.
///
/// Holds the current scroll position and provides methods to programmatically
/// control scrolling. Create with [`rememberLazyListState()`] in composition.
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
/// let state = rememberLazyListState();
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

// Implement PartialEq by comparing the stable inner state handle identity.
// This allows LazyListState to be used as a composable function parameter
// without dereferencing released state cells during parameter updates.
impl PartialEq for LazyListState {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

#[derive(Clone, Copy)]
struct CachedItemSize {
    size: f32,
    last_used: u64,
}

/// Non-reactive internal state for LazyListState.
struct LazyListStateInner {
    /// Scroll delta to be consumed in the next layout pass.
    scroll_to_be_consumed: f32,

    /// Pending scroll-to-item request.
    pending_scroll_to_index: Option<(usize, f32)>,

    /// Layout info from the last measure pass.
    layout_info: LazyListLayoutInfo,
    current_can_scroll_forward: bool,
    current_can_scroll_backward: bool,

    /// Invalidation callbacks.
    invalidate_callbacks: Vec<(u64, Rc<dyn Fn()>)>,
    next_callback_id: u64,

    /// Registered layout invalidation callback id, if any.
    /// Used to prevent duplicate registrations on recomposition and to
    /// allow clean re-registration after a branch is disposed and restored.
    layout_invalidation_callback_id: Option<u64>,
    layout_invalidation_node_id: Option<NodeId>,

    /// Diagnostic counters (non-reactive - not typically displayed in UI).
    total_composed: usize,
    reuse_count: usize,

    /// Cache of recently measured item sizes (index -> main_axis_size).
    item_size_cache: std::collections::HashMap<usize, CachedItemSize>,
    item_size_eviction_queue: BinaryHeap<Reverse<(u64, usize)>>,
    item_size_clock: u64,

    /// Running average of measured item sizes for estimation.
    average_item_size: f32,
    total_measured_items: usize,
    next_measure_cycle_id: u64,
    next_item_measure_pass_id: u64,

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
/// let list_state = rememberLazyListState();
///
/// // Pass to multiple closures - no .clone() needed!
/// LazyColumn(modifier, list_state, spec, content);
/// Button(move || list_state.scroll_to_item(0, 0.0));
/// ```
#[composable]
#[track_caller]
pub fn rememberLazyListState() -> LazyListState {
    rememberLazyListStateWithPosition(0, 0.0)
}

/// Creates a remembered [`LazyListState`] with the specified initial position.
///
/// The returned state is `Copy` and can be passed to multiple closures without `.clone()`.
#[composable]
pub fn rememberLazyListStateWithPosition(
    initial_first_visible_item_index: usize,
    initial_first_visible_item_scroll_offset: f32,
) -> LazyListState {
    // Create scroll position with reactive fields (matches JC LazyListScrollPosition)
    let scroll_position = LazyListScrollPosition {
        index: cranpose_core::rememberMutableStateOf(|| initial_first_visible_item_index),
        scroll_offset: cranpose_core::rememberMutableStateOf(|| {
            initial_first_visible_item_scroll_offset
        }),
        inner: cranpose_core::rememberMutableStateOfNeverEqual(|| {
            Rc::new(RefCell::new(ScrollPositionInner {
                current_index: initial_first_visible_item_index,
                current_scroll_offset: initial_first_visible_item_scroll_offset,
                last_known_first_item_key: None,
                nearest_range_state: NearestRangeState::new(initial_first_visible_item_index),
            }))
        }),
    };

    // Non-reactive internal state
    let inner = cranpose_core::rememberMutableStateOfNeverEqual(|| {
        Rc::new(RefCell::new(LazyListStateInner {
            scroll_to_be_consumed: 0.0,
            pending_scroll_to_index: None,
            layout_info: LazyListLayoutInfo::default(),
            current_can_scroll_forward: false,
            current_can_scroll_backward: false,
            invalidate_callbacks: Vec::new(),
            next_callback_id: 1,
            layout_invalidation_callback_id: None,
            layout_invalidation_node_id: None,
            total_composed: 0,
            reuse_count: 0,
            item_size_cache: std::collections::HashMap::new(),
            item_size_eviction_queue: BinaryHeap::new(),
            item_size_clock: 0,
            average_item_size: super::DEFAULT_ITEM_SIZE_ESTIMATE,
            total_measured_items: 0,
            next_measure_cycle_id: 1,
            next_item_measure_pass_id: 1,
            prefetch_scheduler: PrefetchScheduler::new(),
            prefetch_strategy: PrefetchStrategy::default(),
            last_scroll_direction: 0.0,
        }))
    });

    // Reactive state
    let can_scroll_forward_state = cranpose_core::rememberMutableStateOf(|| false);
    let can_scroll_backward_state = cranpose_core::rememberMutableStateOf(|| false);
    let stats_state = cranpose_core::rememberMutableStateOf(LazyLayoutStats::default);

    LazyListState {
        scroll_position,
        can_scroll_forward_state,
        can_scroll_backward_state,
        stats_state,
        inner,
    }
}

impl LazyListState {
    /// Returns a stable identity pointer for the live inner state allocation.
    ///
    /// The pointer comes from the `Rc` stored inside `inner`, so it remains stable for the
    /// lifetime of a live `LazyListState` and can be used as a composition identity key.
    pub fn inner_ptr(&self) -> *const () {
        self.inner
            .try_with(|rc| Rc::as_ptr(rc) as *const ())
            .unwrap_or(std::ptr::null())
    }

    /// Returns the index of the first visible item.
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes to the index will trigger recomposition.
    pub fn first_visible_item_index(&self) -> usize {
        // Delegate to scroll_position (reactive read)
        self.scroll_position.index()
    }

    /// Returns the first visible item index without subscribing the current composition scope.
    ///
    /// Use this from draw/input/diagnostic code that needs the latest position but must not
    /// recompose when the scroll position changes.
    pub fn first_visible_item_index_non_reactive(&self) -> usize {
        self.scroll_position.current_index()
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

    /// Returns the first visible item scroll offset without subscribing the current composition scope.
    ///
    /// Use this from draw/input/diagnostic code that needs the latest position but must not
    /// recompose when the scroll position changes.
    pub fn first_visible_item_scroll_offset_non_reactive(&self) -> f32 {
        self.scroll_position.current_scroll_offset()
    }

    #[doc(hidden)]
    pub fn reactive_state_ids(&self) -> [StateId; 5] {
        [
            self.scroll_position.index.runtime_state_id(),
            self.scroll_position.scroll_offset.runtime_state_id(),
            self.can_scroll_forward_state.runtime_state_id(),
            self.can_scroll_backward_state.runtime_state_id(),
            self.stats_state.runtime_state_id(),
        ]
    }

    /// Returns the layout info from the last measure pass.
    pub fn layout_info(&self) -> LazyListLayoutInfo {
        self.inner
            .try_with(|rc| rc.borrow().layout_info.clone())
            .unwrap_or_default()
    }

    /// Returns the current item lifecycle statistics.
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes to `items_in_use` or `items_in_pool` will trigger recomposition.
    /// The `total_composed` and `reuse_count` fields are diagnostic and non-reactive.
    pub fn stats(&self) -> LazyLayoutStats {
        if !self.stats_state.is_alive() || !self.inner.is_alive() {
            return LazyLayoutStats::default();
        }
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
        if !self.stats_state.is_alive() || !self.inner.is_alive() {
            return;
        }

        let current = self.stats_state.get_non_reactive();

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
        if !self.inner.is_alive() {
            return;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.total_composed += 1;
            if was_reused {
                inner.reuse_count += 1;
            }
        });
    }

    /// Records the raw scroll delta for prefetch calculations.
    ///
    /// Cranpose lazy lists use gesture-style deltas:
    /// - Negative delta = scrolling forward (content moves up)
    /// - Positive delta = scrolling backward (content moves down)
    pub fn record_scroll_direction(&self, delta: f32) {
        if delta.abs() > 0.001 {
            if !self.inner.is_alive() {
                return;
            }
            self.inner.with(|rc| {
                rc.borrow_mut().last_scroll_direction = -delta.signum();
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
        if !self.inner.is_alive() {
            return;
        }
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
        self.inner
            .try_with(|rc| {
                let mut inner = rc.borrow_mut();
                let mut indices = Vec::new();
                while let Some(idx) = inner.prefetch_scheduler.next_prefetch() {
                    indices.push(idx);
                }
                indices
            })
            .unwrap_or_default()
    }

    /// Scrolls to the specified item index.
    ///
    /// # Arguments
    /// * `index` - The index of the item to scroll to
    /// * `scroll_offset` - Additional offset within the item (default 0)
    pub fn scroll_to_item(&self, index: usize, scroll_offset: f32) {
        if !self.inner.is_alive() {
            return;
        }
        if diagnostics::telemetry_enabled() {
            log::warn!(
                "[lazy-measure-telemetry] scroll_to_item request index={} offset={:.2}",
                index,
                scroll_offset
            );
        }
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
    /// This triggers layout invalidation via registered callbacks. The callbacks
    /// are registered by LazyColumnImpl/LazyRowImpl with
    /// `schedule_measure_repass(node_id)` — the list's own item sizes are what
    /// changes, so the repass has to bubble measure dirtiness, not just
    /// placement. The node id carries through to the scene phase, which scopes
    /// its graph update to that subtree: O(subtree) instead of O(entire app).
    pub fn dispatch_scroll_delta(&self, delta: f32) -> f32 {
        // Guard against stale handles: fling animation frame callbacks can fire
        // after a tab switch disposes the composition group that owns this state.
        if !self.inner.is_alive() {
            return 0.0;
        }
        let has_scroll_bounds = self
            .inner
            .with(|rc| rc.borrow().layout_info.total_items_count > 0);
        let pushing_forward = delta < -0.001;
        let pushing_backward = delta > 0.001;
        let can_scroll_forward =
            self.can_scroll_forward_state.is_alive() && self.can_scroll_forward_non_reactive();
        let can_scroll_backward =
            self.can_scroll_backward_state.is_alive() && self.can_scroll_backward_non_reactive();
        let blocked_by_bounds = has_scroll_bounds
            && ((pushing_forward && !can_scroll_forward)
                || (pushing_backward && !can_scroll_backward));

        if blocked_by_bounds {
            let should_invalidate = self.inner.with(|rc| {
                let mut inner = rc.borrow_mut();
                let pending_before = inner.scroll_to_be_consumed;
                // If we're already at an edge, clear stale backlog in the same blocked direction.
                if pending_before.abs() > 0.001 && pending_before.signum() == delta.signum() {
                    inner.scroll_to_be_consumed = 0.0;
                }
                if diagnostics::telemetry_enabled() {
                    log::warn!(
                        "[lazy-measure-telemetry] dispatch_scroll_delta blocked_by_bounds delta={:.2} pending_before={:.2} pending_after={:.2}",
                        delta,
                        pending_before,
                        inner.scroll_to_be_consumed
                    );
                }
                (inner.scroll_to_be_consumed - pending_before).abs() > 0.001
            });
            if should_invalidate {
                self.invalidate();
            }
            return 0.0;
        }

        let mut accepted_delta = 0.0f32;
        let should_invalidate = self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            accepted_delta = delta;
            let pending_before = inner.scroll_to_be_consumed;
            let pending = inner.scroll_to_be_consumed;
            let reverse_input = pending.abs() > 0.001
                && delta.abs() > 0.001
                && pending.signum() != delta.signum();
            if reverse_input {
                if diagnostics::telemetry_enabled() {
                    log::warn!(
                        "[lazy-measure-telemetry] dispatch_scroll_delta direction_change pending={:.2} new_delta={:.2}",
                        pending,
                        delta
                    );
                }
                // When gesture direction reverses, stale unconsumed backlog from the previous
                // direction causes "snap back" behavior on slow frames. Keep only the latest
                // direction intent.
                inner.scroll_to_be_consumed = delta;
            } else {
                inner.scroll_to_be_consumed += delta;
            }
            inner.scroll_to_be_consumed = inner
                .scroll_to_be_consumed
                .clamp(-MAX_PENDING_SCROLL_DELTA, MAX_PENDING_SCROLL_DELTA);
            if diagnostics::telemetry_enabled() {
                log::warn!(
                    "[lazy-measure-telemetry] dispatch_scroll_delta delta={:.2} pending={:.2}",
                    delta,
                    inner.scroll_to_be_consumed
                );
            }
            (inner.scroll_to_be_consumed - pending_before).abs() > 0.001
        });
        if should_invalidate {
            self.invalidate();
        }
        accepted_delta
    }

    /// Peeks at the pending scroll delta without consuming it.
    ///
    /// Used for direction inference before measurement consumes the delta.
    /// This is more accurate than comparing first visible index, especially for:
    /// - Scrolling within the same item (partial scroll)
    /// - Variable height items where scroll offset changes without index change
    pub fn peek_scroll_delta(&self) -> f32 {
        self.inner
            .try_with(|rc| rc.borrow().scroll_to_be_consumed)
            .unwrap_or(0.0)
    }

    pub(crate) fn begin_measure_pass(&self) -> LazyListMeasureStateSnapshot {
        let (pending_scroll_delta, pending_scroll_to, average_item_size) = self
            .inner
            .try_with(|rc| {
                let mut inner = rc.borrow_mut();
                let pending_scroll_to = inner.pending_scroll_to_index.take();
                let pending_scroll_delta = inner.scroll_to_be_consumed;
                inner.scroll_to_be_consumed = 0.0;
                (
                    pending_scroll_delta,
                    pending_scroll_to,
                    inner.average_item_size,
                )
            })
            .unwrap_or((0.0, None, super::DEFAULT_ITEM_SIZE_ESTIMATE));

        LazyListMeasureStateSnapshot {
            first_visible_item_index: self.scroll_position.current_index(),
            first_visible_item_scroll_offset: self.scroll_position.current_scroll_offset(),
            pending_scroll_delta,
            pending_scroll_to,
            average_item_size,
        }
    }

    pub(crate) fn next_measure_cycle_id(&self) -> u64 {
        self.inner
            .try_with(|rc| {
                let mut inner = rc.borrow_mut();
                let id = inner.next_measure_cycle_id;
                inner.next_measure_cycle_id = inner.next_measure_cycle_id.saturating_add(1);
                id
            })
            .unwrap_or(0)
    }

    pub(crate) fn next_item_measure_pass_id(&self) -> u64 {
        self.inner
            .try_with(|rc| {
                let mut inner = rc.borrow_mut();
                let id = inner.next_item_measure_pass_id;
                inner.next_item_measure_pass_id = inner.next_item_measure_pass_id.saturating_add(1);
                id
            })
            .unwrap_or(0)
    }

    fn record_item_size_sample(inner: &mut LazyListStateInner, size: f32) {
        inner.total_measured_items += 1;
        let n = inner.total_measured_items as f32;
        inner.average_item_size = inner.average_item_size * ((n - 1.0) / n) + size / n;
    }

    fn next_item_size_cache_tick(inner: &mut LazyListStateInner) -> u64 {
        inner.item_size_clock = inner.item_size_clock.saturating_add(1);
        inner.item_size_clock
    }

    fn insert_item_size(inner: &mut LazyListStateInner, index: usize, size: f32) -> bool {
        use std::collections::hash_map::Entry;

        let tick = Self::next_item_size_cache_tick(inner);
        if let Entry::Occupied(mut entry) = inner.item_size_cache.entry(index) {
            entry.insert(CachedItemSize {
                size,
                last_used: tick,
            });
            Self::push_item_size_cache_ticket(inner, tick, index);
            return false;
        }

        if inner.item_size_cache.len() >= ITEM_SIZE_CACHE_CAPACITY {
            Self::evict_one_item_size(inner);
        }

        inner.item_size_cache.insert(
            index,
            CachedItemSize {
                size,
                last_used: tick,
            },
        );
        Self::push_item_size_cache_ticket(inner, tick, index);
        true
    }

    fn push_item_size_cache_ticket(inner: &mut LazyListStateInner, last_used: u64, index: usize) {
        inner
            .item_size_eviction_queue
            .push(Reverse((last_used, index)));
        let compact_limit = inner
            .item_size_cache
            .len()
            .saturating_mul(4)
            .max(ITEM_SIZE_CACHE_CAPACITY);
        if inner.item_size_eviction_queue.len() > compact_limit {
            Self::rebuild_item_size_eviction_queue(inner);
        }
    }

    fn rebuild_item_size_eviction_queue(inner: &mut LazyListStateInner) {
        inner.item_size_eviction_queue = inner
            .item_size_cache
            .iter()
            .map(|(index, item)| Reverse((item.last_used, *index)))
            .collect();
    }

    fn evict_one_item_size(inner: &mut LazyListStateInner) {
        while let Some(Reverse((last_used, index))) = inner.item_size_eviction_queue.pop() {
            let Some(current) = inner.item_size_cache.get(&index) else {
                continue;
            };
            if current.last_used != last_used {
                continue;
            }
            inner.item_size_cache.remove(&index);
            return;
        }
    }

    /// Caches the measured size of an item for scroll estimation.
    pub fn cache_item_size(&self, index: usize, size: f32) {
        if !self.inner.is_alive() {
            return;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            if Self::insert_item_size(&mut inner, index, size) {
                Self::record_item_size_sample(&mut inner, size);
            }
        });
    }

    /// Caches multiple measured item sizes in one pass and returns the updated average.
    pub fn cache_item_sizes<I>(&self, sizes: I) -> f32
    where
        I: IntoIterator<Item = (usize, f32)>,
    {
        if !self.inner.is_alive() {
            return super::DEFAULT_ITEM_SIZE_ESTIMATE;
        }

        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            for (index, size) in sizes {
                if Self::insert_item_size(&mut inner, index, size) {
                    Self::record_item_size_sample(&mut inner, size);
                }
            }
            inner.average_item_size
        })
    }

    /// Gets a cached item size if available.
    pub fn get_cached_size(&self, index: usize) -> Option<f32> {
        self.inner
            .try_with(|rc| {
                let mut inner = rc.borrow_mut();
                let tick = Self::next_item_size_cache_tick(&mut inner);
                let item = inner.item_size_cache.get_mut(&index)?;
                item.last_used = tick;
                let size = item.size;
                Self::push_item_size_cache_ticket(&mut inner, tick, index);
                Some(size)
            })
            .flatten()
    }

    /// Returns the running average of measured item sizes.
    pub fn average_item_size(&self) -> f32 {
        self.inner
            .try_with(|rc| rc.borrow().average_item_size)
            .unwrap_or(super::DEFAULT_ITEM_SIZE_ESTIMATE)
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
    pub(crate) fn update_layout_info(&self, mut info: LazyListLayoutInfo) {
        if !self.inner.is_alive() {
            return;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            info.snap_anchor_offset = continuous_snap_anchor_offset(&inner.layout_info, &info);
            inner.layout_info = info;
        });
    }

    /// Returns whether we can scroll forward (more items below/right).
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes will trigger recomposition.
    pub fn can_scroll_forward(&self) -> bool {
        if !self.can_scroll_forward_state.is_alive() {
            return false;
        }
        self.can_scroll_forward_state.subscribe_current_scope_only();
        self.can_scroll_forward_non_reactive()
    }

    /// Returns whether the list can scroll forward without subscribing the current composition scope.
    pub fn can_scroll_forward_non_reactive(&self) -> bool {
        if !self.can_scroll_forward_state.is_alive() {
            return false;
        }
        self.inner
            .try_with(|rc| rc.borrow().current_can_scroll_forward)
            .unwrap_or(false)
    }

    /// Returns whether we can scroll backward (more items above/left).
    ///
    /// When called during composition, this creates a reactive subscription
    /// so that changes will trigger recomposition.
    pub fn can_scroll_backward(&self) -> bool {
        if !self.can_scroll_backward_state.is_alive() {
            return false;
        }
        self.can_scroll_backward_state
            .subscribe_current_scope_only();
        self.can_scroll_backward_non_reactive()
    }

    /// Returns whether the list can scroll backward without subscribing the current composition scope.
    pub fn can_scroll_backward_non_reactive(&self) -> bool {
        if !self.can_scroll_backward_state.is_alive() {
            return false;
        }
        self.inner
            .try_with(|rc| rc.borrow().current_can_scroll_backward)
            .unwrap_or(false)
    }

    /// Updates the scroll bounds after layout measurement.
    ///
    /// Called by the layout after measurement to update can_scroll_forward/backward.
    pub(crate) fn update_scroll_bounds(&self) {
        if !self.inner.is_alive()
            || !self.can_scroll_forward_state.is_alive()
            || !self.can_scroll_backward_state.is_alive()
        {
            return;
        }
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
        let can_backward = self.scroll_position.current_index() > 0
            || self.scroll_position.current_scroll_offset() > 0.0;

        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.current_can_scroll_forward = can_forward;
            inner.current_can_scroll_backward = can_backward;
        });

        if self.can_scroll_forward_state.get_non_reactive() != can_forward {
            self.can_scroll_forward_state.set(can_forward);
        }
        if self.can_scroll_backward_state.get_non_reactive() != can_backward {
            self.can_scroll_backward_state.set(can_backward);
        }
    }

    /// Adds an invalidation callback.
    pub fn add_invalidate_callback(&self, callback: Rc<dyn Fn()>) -> u64 {
        if !self.inner.is_alive() {
            return 0;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            let id = inner.next_callback_id;
            inner.next_callback_id += 1;
            inner.invalidate_callbacks.push((id, callback));
            id
        })
    }

    /// Tries to register a layout invalidation callback for the specified node.
    ///
    /// Returns the callback id for the active layout callback.
    ///
    /// Registering again always replaces the previous active layout callback, even when
    /// the node id stays the same. This keeps ownership tied to the latest effect
    /// instance so disposing an older scope cannot unregister the live callback.
    pub fn try_register_layout_callback(
        &self,
        node_id: NodeId,
        callback: Rc<dyn Fn()>,
    ) -> Option<u64> {
        if !self.inner.is_alive() {
            return None;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            if let Some(existing_id) = inner.layout_invalidation_callback_id {
                inner
                    .invalidate_callbacks
                    .retain(|(cb_id, _)| *cb_id != existing_id);
            }
            let id = inner.next_callback_id;
            inner.next_callback_id += 1;
            inner.invalidate_callbacks.push((id, callback));
            inner.layout_invalidation_callback_id = Some(id);
            inner.layout_invalidation_node_id = Some(node_id);
            Some(id)
        })
    }

    /// Removes an invalidation callback.
    pub fn remove_invalidate_callback(&self, id: u64) {
        if !self.inner.is_alive() {
            return;
        }
        self.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.invalidate_callbacks.retain(|(cb_id, _)| *cb_id != id);
            if inner.layout_invalidation_callback_id == Some(id) {
                inner.layout_invalidation_callback_id = None;
                inner.layout_invalidation_node_id = None;
            }
        });
    }

    fn invalidate(&self) {
        if !self.inner.is_alive() {
            return;
        }
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

    /// Raw viewport size reported by parent constraints (before infinite fallback).
    pub raw_viewport_size: f32,

    /// Whether the viewport was treated as infinite/unbounded.
    pub is_infinite_viewport: bool,

    /// Size of the viewport in the main axis.
    pub viewport_size: f32,

    /// Start offset of the viewport in layout coordinates.
    pub viewport_start_offset: f32,

    /// End offset of the viewport in layout coordinates.
    pub viewport_end_offset: f32,

    /// Content padding before the first item.
    pub before_content_padding: f32,

    /// Content padding after the last item.
    pub after_content_padding: f32,

    /// Continuous main-axis visual offset used to snap translated lazy-list content.
    pub snap_anchor_offset: f32,

    /// Whether item offsets are placed from the end edge of the viewport.
    pub reverse_layout: bool,
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

fn continuous_snap_anchor_offset(
    previous: &LazyListLayoutInfo,
    current: &LazyListLayoutInfo,
) -> f32 {
    let Some(first_current) = current.visible_items_info.first() else {
        return 0.0;
    };

    for current_item in &current.visible_items_info {
        if let Some(previous_item) = previous
            .visible_items_info
            .iter()
            .find(|item| item.key == current_item.key)
        {
            let previous_offset = snap_anchor_item_offset(previous, previous_item);
            let current_offset = snap_anchor_item_offset(current, current_item);
            return previous.snap_anchor_offset + current_offset - previous_offset;
        }
    }

    snap_anchor_item_offset(current, first_current)
}

fn snap_anchor_item_offset(info: &LazyListLayoutInfo, item: &LazyListItemInfo) -> f32 {
    if info.reverse_layout {
        info.viewport_size - item.offset - item.size
    } else {
        item.offset
    }
}

/// Test helpers for creating LazyListState without composition context.
#[cfg(test)]
pub mod test_helpers {
    use std::sync::Arc;

    use cranpose_core::{DefaultScheduler, Runtime};

    use super::*;

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
            index: cranpose_core::mutableStateOf(initial_first_visible_item_index),
            scroll_offset: cranpose_core::mutableStateOf(initial_first_visible_item_scroll_offset),
            inner: cranpose_core::mutableStateOf(Rc::new(RefCell::new(ScrollPositionInner {
                current_index: initial_first_visible_item_index,
                current_scroll_offset: initial_first_visible_item_scroll_offset,
                last_known_first_item_key: None,
                nearest_range_state: NearestRangeState::new(initial_first_visible_item_index),
            }))),
        };

        // Non-reactive internal state
        let inner = cranpose_core::mutableStateOf(Rc::new(RefCell::new(LazyListStateInner {
            scroll_to_be_consumed: 0.0,
            pending_scroll_to_index: None,
            layout_info: LazyListLayoutInfo::default(),
            current_can_scroll_forward: false,
            current_can_scroll_backward: false,
            invalidate_callbacks: Vec::new(),
            next_callback_id: 1,
            layout_invalidation_callback_id: None,
            layout_invalidation_node_id: None,
            total_composed: 0,
            reuse_count: 0,
            item_size_cache: std::collections::HashMap::new(),
            item_size_eviction_queue: BinaryHeap::new(),
            item_size_clock: 0,
            average_item_size: super::super::DEFAULT_ITEM_SIZE_ESTIMATE,
            total_measured_items: 0,
            next_measure_cycle_id: 1,
            next_item_measure_pass_id: 1,
            prefetch_scheduler: PrefetchScheduler::new(),
            prefetch_strategy: PrefetchStrategy::default(),
            last_scroll_direction: 0.0,
        })));

        // Reactive state
        let can_scroll_forward_state = cranpose_core::mutableStateOf(false);
        let can_scroll_backward_state = cranpose_core::mutableStateOf(false);
        let stats_state = cranpose_core::mutableStateOf(LazyLayoutStats::default());

        LazyListState {
            scroll_position,
            can_scroll_forward_state,
            can_scroll_backward_state,
            stats_state,
            inner,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{Composition, MemoryApplier, location_key};

    use super::{
        LazyListItemInfo, LazyListLayoutInfo, LazyListState,
        test_helpers::{new_lazy_list_state, new_lazy_list_state_with_position, with_test_runtime},
    };

    fn set_scroll_bounds(state: &LazyListState, can_forward: bool, can_backward: bool) {
        state.can_scroll_forward_state.set(can_forward);
        state.can_scroll_backward_state.set(can_backward);
        state.inner.with(|rc| {
            let mut inner = rc.borrow_mut();
            inner.current_can_scroll_forward = can_forward;
            inner.current_can_scroll_backward = can_backward;
        });
    }

    fn enable_bidirectional_scroll(state: &LazyListState) {
        set_scroll_bounds(state, true, true);
    }

    fn mark_scroll_bounds_known(state: &LazyListState) {
        state.update_layout_info(LazyListLayoutInfo {
            total_items_count: 10,
            ..Default::default()
        });
    }

    fn visible_item(index: usize, offset: f32, size: f32) -> LazyListItemInfo {
        LazyListItemInfo {
            index,
            key: index as u64,
            offset,
            size,
        }
    }

    #[test]
    fn lazy_measure_telemetry_ids_are_state_owned() {
        with_test_runtime(|| {
            let first = new_lazy_list_state();
            let second = new_lazy_list_state();

            assert_eq!(first.next_measure_cycle_id(), 1);
            assert_eq!(first.next_measure_cycle_id(), 2);
            assert_eq!(second.next_measure_cycle_id(), 1);

            assert_eq!(first.next_item_measure_pass_id(), 1);
            assert_eq!(first.next_item_measure_pass_id(), 2);
            assert_eq!(second.next_item_measure_pass_id(), 1);
        });
    }

    #[test]
    fn measure_result_updates_retained_and_reactive_scroll_position() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();

            state.update_scroll_position_with_key(8, 17.5, 123);

            assert_eq!(state.scroll_position.index.get_non_reactive(), 8);
            assert!((state.scroll_position.scroll_offset.get_non_reactive() - 17.5).abs() < 0.001);
            assert_eq!(state.first_visible_item_index_non_reactive(), 8);
            assert!((state.first_visible_item_scroll_offset_non_reactive() - 17.5).abs() < 0.001);
        });
    }

    #[test]
    fn update_scroll_bounds_updates_retained_and_reactive_capabilities() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();

            state.update_layout_info(LazyListLayoutInfo {
                visible_items_info: vec![visible_item(0, 0.0, 40.0), visible_item(1, 40.0, 40.0)],
                total_items_count: 10,
                viewport_size: 80.0,
                ..Default::default()
            });
            state.update_scroll_bounds();

            assert!(state.can_scroll_forward_state.get_non_reactive());
            assert!(!state.can_scroll_backward_state.get_non_reactive());
            assert!(state.can_scroll_forward_non_reactive());
            assert!(!state.can_scroll_backward_non_reactive());

            state.update_scroll_position(3, 2.0);
            state.update_scroll_bounds();

            assert!(state.can_scroll_backward_state.get_non_reactive());
            assert!(state.can_scroll_backward_non_reactive());
        });
    }

    #[test]
    fn layout_info_snap_anchor_tracks_common_item_offset_delta() {
        let previous = LazyListLayoutInfo {
            visible_items_info: vec![visible_item(15, -31.4, 30.0), visible_item(16, 4.6, 30.0)],
            snap_anchor_offset: -31.4,
            ..Default::default()
        };
        let current = LazyListLayoutInfo {
            visible_items_info: vec![visible_item(16, 3.6, 30.0), visible_item(17, 39.6, 30.0)],
            ..Default::default()
        };

        let anchor = super::continuous_snap_anchor_offset(&previous, &current);

        assert!((anchor + 32.4).abs() <= 0.001);
    }

    #[test]
    fn layout_info_snap_anchor_uses_reverse_visual_item_offset() {
        let previous = LazyListLayoutInfo {
            visible_items_info: vec![visible_item(15, 31.4, 30.0), visible_item(16, 67.4, 30.0)],
            snap_anchor_offset: 58.6,
            viewport_size: 120.0,
            reverse_layout: true,
            ..Default::default()
        };
        let current = LazyListLayoutInfo {
            visible_items_info: vec![visible_item(16, 68.4, 30.0), visible_item(17, 104.4, 30.0)],
            viewport_size: 120.0,
            reverse_layout: true,
            ..Default::default()
        };

        let anchor = super::continuous_snap_anchor_offset(&previous, &current);

        assert!((anchor - 57.6).abs() <= 0.001);
    }

    #[test]
    fn update_layout_info_keeps_snap_anchor_continuous_when_first_visible_item_changes() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            state.update_layout_info(LazyListLayoutInfo {
                visible_items_info: vec![
                    visible_item(15, -31.4, 30.0),
                    visible_item(16, 4.6, 30.0),
                ],
                ..Default::default()
            });

            state.update_layout_info(LazyListLayoutInfo {
                visible_items_info: vec![visible_item(16, 3.6, 30.0), visible_item(17, 39.6, 30.0)],
                ..Default::default()
            });

            let info = state.layout_info();
            assert!((info.snap_anchor_offset + 32.4).abs() <= 0.001);
        });
    }

    #[test]
    fn dispatch_scroll_delta_accumulates_same_direction() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            enable_bidirectional_scroll(&state);

            state.dispatch_scroll_delta(-12.0);
            state.dispatch_scroll_delta(-8.0);

            assert!((state.peek_scroll_delta() + 20.0).abs() < 0.001);
            let snapshot = state.begin_measure_pass();
            assert!((snapshot.pending_scroll_delta + 20.0).abs() < 0.001);
            assert_eq!(state.begin_measure_pass().pending_scroll_delta, 0.0);
        });
    }

    #[test]
    fn dispatch_scroll_delta_drops_stale_backlog_on_direction_change() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            enable_bidirectional_scroll(&state);

            state.dispatch_scroll_delta(-120.0);
            state.dispatch_scroll_delta(-30.0);
            assert!((state.peek_scroll_delta() + 150.0).abs() < 0.001);

            state.dispatch_scroll_delta(18.0);

            assert!((state.peek_scroll_delta() - 18.0).abs() < 0.001);
            let snapshot = state.begin_measure_pass();
            assert!((snapshot.pending_scroll_delta - 18.0).abs() < 0.001);
            assert_eq!(state.begin_measure_pass().pending_scroll_delta, 0.0);
        });
    }

    #[test]
    fn dispatch_scroll_delta_clamps_pending_backlog() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            enable_bidirectional_scroll(&state);

            state.dispatch_scroll_delta(-1_500.0);
            state.dispatch_scroll_delta(-1_500.0);
            assert!((state.peek_scroll_delta() + super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);

            state.dispatch_scroll_delta(3_000.0);
            assert!((state.peek_scroll_delta() - super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);
        });
    }

    #[test]
    fn begin_measure_pass_consumes_large_pending_scroll_delta_coherently() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            enable_bidirectional_scroll(&state);
            let invalidations = Rc::new(Cell::new(0u32));
            let invalidations_clone = Rc::clone(&invalidations);
            state.add_invalidate_callback(Rc::new(move || {
                invalidations_clone.set(invalidations_clone.get() + 1);
            }));

            state.dispatch_scroll_delta(-1_000.0);
            assert!((state.peek_scroll_delta() + 1_000.0).abs() < 0.001);

            let first = state.begin_measure_pass();
            assert!(
                (first.pending_scroll_delta + 1_000.0).abs() < 0.001,
                "first pass should consume the whole coherent scroll input"
            );
            assert!(
                state.peek_scroll_delta().abs() < 0.001,
                "measure pass should not retain a synthetic scroll backlog"
            );
            assert_eq!(
                invalidations.get(),
                1,
                "dispatch should request layout once; consuming scroll should not schedule follow-up frames"
            );

            let second = state.begin_measure_pass();
            assert!(
                second.pending_scroll_delta.abs() < 0.001,
                "second pass should not receive synthetic remainder"
            );
        });
    }

    #[test]
    fn dispatch_scroll_delta_skips_invalidate_when_clamped_value_is_unchanged() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            enable_bidirectional_scroll(&state);
            let invalidations = Rc::new(Cell::new(0u32));
            let invalidations_clone = Rc::clone(&invalidations);
            state.add_invalidate_callback(Rc::new(move || {
                invalidations_clone.set(invalidations_clone.get() + 1);
            }));

            state.dispatch_scroll_delta(-3_000.0);
            assert_eq!(invalidations.get(), 1);
            assert!((state.peek_scroll_delta() + super::MAX_PENDING_SCROLL_DELTA).abs() < 0.001);

            // Additional same-direction input is clamped to the same pending value.
            state.dispatch_scroll_delta(-100.0);
            assert_eq!(invalidations.get(), 1);

            // Opposite-direction input changes pending and should invalidate again.
            state.dispatch_scroll_delta(100.0);
            assert_eq!(invalidations.get(), 2);
        });
    }

    #[test]
    fn begin_measure_pass_takes_coherent_snapshot_and_consumes_pending_inputs() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(3, 12.0);
            state.dispatch_scroll_delta(-20.0);
            state.inner.with(|rc| {
                rc.borrow_mut().pending_scroll_to_index = Some((8, 4.0));
            });

            let snapshot = state.begin_measure_pass();

            assert_eq!(snapshot.first_visible_item_index, 3);
            assert!((snapshot.first_visible_item_scroll_offset - 12.0).abs() < 0.001);
            assert!((snapshot.pending_scroll_delta + 20.0).abs() < 0.001);
            assert_eq!(snapshot.pending_scroll_to, Some((8, 4.0)));
            assert_eq!(state.peek_scroll_delta(), 0.0);
            assert_eq!(state.begin_measure_pass().pending_scroll_to, None);
        });
    }

    #[test]
    fn item_size_cache_refresh_keeps_recent_entry_and_evicts_oldest_live_entry() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            for index in 0..super::ITEM_SIZE_CACHE_CAPACITY {
                state.cache_item_size(index, index as f32 + 10.0);
            }

            state.cache_item_size(0, 999.0);
            state.cache_item_size(super::ITEM_SIZE_CACHE_CAPACITY, 123.0);

            assert_eq!(state.get_cached_size(0), Some(999.0));
            assert_eq!(state.get_cached_size(1), None);
            assert_eq!(
                state.get_cached_size(super::ITEM_SIZE_CACHE_CAPACITY),
                Some(123.0),
            );
        });
    }

    #[test]
    fn item_size_cache_read_promotes_entry_for_large_scroll_reuse() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            for index in 0..super::ITEM_SIZE_CACHE_CAPACITY {
                state.cache_item_size(index, index as f32 + 10.0);
            }

            assert_eq!(state.get_cached_size(0), Some(10.0));
            state.cache_item_size(super::ITEM_SIZE_CACHE_CAPACITY, 123.0);

            assert_eq!(state.get_cached_size(0), Some(10.0));
            assert_eq!(state.get_cached_size(1), None);
            let cache_len = state
                .inner
                .try_with(|rc| rc.borrow().item_size_cache.len())
                .unwrap_or(0);
            assert_eq!(cache_len, super::ITEM_SIZE_CACHE_CAPACITY);
        });
    }

    #[test]
    fn item_size_cache_promotion_queue_stays_bounded_under_hot_reuse() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            state.cache_item_size(0, 32.0);

            for _ in 0..super::ITEM_SIZE_CACHE_CAPACITY * 8 {
                assert_eq!(state.get_cached_size(0), Some(32.0));
            }

            let (cache_len, queue_len) = state
                .inner
                .try_with(|rc| {
                    let inner = rc.borrow();
                    (
                        inner.item_size_cache.len(),
                        inner.item_size_eviction_queue.len(),
                    )
                })
                .unwrap_or((0, 0));
            assert_eq!(cache_len, 1);
            assert!(
                queue_len <= super::ITEM_SIZE_CACHE_CAPACITY,
                "stale promotion tickets must be compacted, got {queue_len}"
            );
        });
    }

    #[test]
    fn cache_item_sizes_updates_average_only_for_new_entries() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();

            let average = state.cache_item_sizes([(0, 10.0), (1, 20.0), (0, 12.0)]);

            assert_eq!(state.get_cached_size(0), Some(12.0));
            assert_eq!(state.get_cached_size(1), Some(20.0));
            assert!((average - 15.0).abs() < 0.001);
        });
    }

    #[test]
    fn layout_callback_can_be_registered_again_after_removal() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let first_node: cranpose_core::NodeId = 1;
            let second_node: cranpose_core::NodeId = 2;

            let first_id = state
                .try_register_layout_callback(first_node, Rc::new(|| {}))
                .expect("first layout callback should register");
            let duplicate_id = state
                .try_register_layout_callback(first_node, Rc::new(|| {}))
                .expect("duplicate register should replace with a fresh callback id");
            assert_eq!(
                state
                    .inner
                    .with(|rc| rc.borrow().layout_invalidation_callback_id),
                Some(duplicate_id),
                "duplicate registration should become the active callback",
            );
            assert_ne!(
                first_id, duplicate_id,
                "duplicate registration should replace the old callback id",
            );

            state.remove_invalidate_callback(first_id);

            let second_id = state
                .try_register_layout_callback(second_node, Rc::new(|| {}))
                .expect("layout callback should register again after removal");
            assert_ne!(first_id, second_id);
        });
    }

    #[test]
    fn layout_callback_rebinds_when_node_id_changes() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let first_node: cranpose_core::NodeId = 11;
            let second_node: cranpose_core::NodeId = 22;

            let first_id = state
                .try_register_layout_callback(first_node, Rc::new(|| {}))
                .expect("first layout callback should register");

            let second_id = state
                .try_register_layout_callback(second_node, Rc::new(|| {}))
                .expect("layout callback should rebind to a new node");

            assert_ne!(first_id, second_id);
        });
    }

    #[test]
    fn stale_layout_callback_disposer_cannot_remove_replaced_same_node_callback() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let node_id: cranpose_core::NodeId = 7;
            let first_hits = Rc::new(Cell::new(0u32));
            let second_hits = Rc::new(Cell::new(0u32));

            let first_id = state
                .try_register_layout_callback(
                    node_id,
                    Rc::new({
                        let first_hits = Rc::clone(&first_hits);
                        move || first_hits.set(first_hits.get() + 1)
                    }),
                )
                .expect("first layout callback should register");

            let second_id = state
                .try_register_layout_callback(
                    node_id,
                    Rc::new({
                        let second_hits = Rc::clone(&second_hits);
                        move || second_hits.set(second_hits.get() + 1)
                    }),
                )
                .expect("same-node registration should replace the active callback");

            assert_ne!(first_id, second_id);

            state.remove_invalidate_callback(first_id);
            state.dispatch_scroll_delta(-12.0);

            assert_eq!(
                first_hits.get(),
                0,
                "replaced callback should not be invoked after removal",
            );
            assert_eq!(
                second_hits.get(),
                1,
                "active callback should survive stale disposer cleanup",
            );
        });
    }

    #[test]
    fn dispatch_scroll_delta_returns_zero_when_forward_is_blocked() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            mark_scroll_bounds_known(&state);
            set_scroll_bounds(&state, false, true);

            let consumed = state.dispatch_scroll_delta(-24.0);

            assert_eq!(consumed, 0.0);
            assert_eq!(state.peek_scroll_delta(), 0.0);
        });
    }

    #[test]
    fn equality_does_not_deref_released_inner_state() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        let mut first = None;
        composition
            .render(key, || {
                first = Some(super::rememberLazyListState());
            })
            .expect("initial render");
        let first = first.expect("first lazy state");

        composition
            .render(key, || {})
            .expect("dispose first lazy state");
        assert!(
            !first.inner.is_alive(),
            "expected first lazy state to be released after disposal"
        );

        let mut second = None;
        composition
            .render(key, || {
                second = Some(super::rememberLazyListState());
            })
            .expect("second render");
        let second = second.expect("second lazy state");

        assert!(
            first != second,
            "released lazy state handle must compare by identity without panicking"
        );
    }

    #[test]
    fn released_lazy_list_state_scroll_position_methods_do_not_panic() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        let mut released = None;
        composition
            .render(key, || {
                released = Some(super::rememberLazyListState());
            })
            .expect("initial render");
        let released = released.expect("lazy list state");

        composition
            .render(key, || {})
            .expect("dispose lazy list state");
        assert!(
            !released.inner.is_alive(),
            "expected lazy list state to be released after disposal"
        );

        assert_eq!(released.first_visible_item_index(), 0);
        assert_eq!(released.first_visible_item_scroll_offset(), 0.0);
        assert_eq!(released.nearest_range(), 0..0);
        assert_eq!(
            released.update_scroll_position_if_item_moved(10, |_| Some(0)),
            0
        );
        released.update_scroll_position(3, 12.0);
        released.update_scroll_position_with_key(3, 12.0, 42);
        released.update_scroll_bounds();
    }

    #[test]
    fn dispatch_scroll_delta_clears_stale_pending_at_forward_edge() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            mark_scroll_bounds_known(&state);
            enable_bidirectional_scroll(&state);
            state.dispatch_scroll_delta(-300.0);
            assert!((state.peek_scroll_delta() + 300.0).abs() < 0.001);

            set_scroll_bounds(&state, false, true);

            let blocked_consumed = state.dispatch_scroll_delta(-10.0);
            assert_eq!(blocked_consumed, 0.0);
            assert_eq!(state.peek_scroll_delta(), 0.0);

            let reverse_consumed = state.dispatch_scroll_delta(12.0);
            assert_eq!(reverse_consumed, 12.0);
            assert!((state.peek_scroll_delta() - 12.0).abs() < 0.001);
        });
    }

    #[test]
    fn negative_scroll_delta_prefetches_forward_items() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            state.dispatch_scroll_delta(-24.0);
            state.record_scroll_direction(state.peek_scroll_delta());
            state.update_prefetch_queue(10, 15, 100);

            assert_eq!(state.take_prefetch_indices(), vec![16, 17]);
        });
    }
}
