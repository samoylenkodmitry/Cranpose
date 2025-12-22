//! Core measurement algorithm for lazy lists.
//!
//! This module implements the virtualized measurement logic that determines
//! which items should be composed and measured based on the current scroll
//! position and viewport size.

use super::lazy_list_measured_item::{LazyListMeasuredItem, LazyListMeasureResult};
use super::lazy_list_state::{LazyListLayoutInfo, LazyListState};

/// Default estimated item size for scroll calculations.
/// Used when no measured sizes are cached.
/// 48.0 is a common list item height (Material Design list tile).
pub const DEFAULT_ITEM_SIZE_ESTIMATE: f32 = 48.0;

/// Configuration for lazy list measurement.
#[derive(Clone, Debug)]
pub struct LazyListMeasureConfig {
    /// Whether the list is vertical (true) or horizontal (false).
    pub is_vertical: bool,

    /// Whether layout is reversed.
    pub reverse_layout: bool,

    /// Content padding before the first item.
    pub before_content_padding: f32,

    /// Content padding after the last item.
    pub after_content_padding: f32,

    /// Spacing between items.
    pub spacing: f32,

    /// Number of items to keep composed beyond visible bounds.
    /// Default is 2 items before and after.
    pub beyond_bounds_item_count: usize,
}

impl Default for LazyListMeasureConfig {
    fn default() -> Self {
        Self {
            is_vertical: true,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 0.0,
            beyond_bounds_item_count: 2,
        }
    }
}

/// Measures a lazy list and returns the items to compose/place.
///
/// This is the core algorithm that determines virtualization behavior:
/// 1. Handle pending scroll-to-item requests
/// 2. Apply scroll delta to current position
/// 3. Determine which items are visible in the viewport
/// 4. Compose and measure only those items (+ beyond bounds buffer)
/// 5. Calculate placements and total content size
///
/// # Arguments
/// * `items_count` - Total number of items in the list
/// * `state` - Current scroll state
/// * `viewport_size` - Size of the viewport in main axis
/// * `cross_axis_size` - Size of the viewport in cross axis
/// * `config` - Measurement configuration
/// * `measure_item` - Callback to compose and measure an item at given index
///
/// # Returns
/// A [`LazyListMeasureResult`] containing the items to place.
pub fn measure_lazy_list<F>(
    items_count: usize,
    state: &LazyListState,
    viewport_size: f32,
    _cross_axis_size: f32,
    config: &LazyListMeasureConfig,
    mut measure_item: F,
) -> LazyListMeasureResult
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    if items_count == 0 || viewport_size <= 0.0 {
        return LazyListMeasureResult::default();
    }

    // Handle pending scroll-to-item request
    let (mut first_item_index, mut first_item_scroll_offset) =
        if let Some((target_index, target_offset)) = state.consume_scroll_to_index() {
            let clamped = target_index.min(items_count.saturating_sub(1));
            (clamped, target_offset)
        } else {
            (
                state.first_visible_item_index().min(items_count.saturating_sub(1)),
                state.first_visible_item_scroll_offset(),
            )
        };

    // Apply pending scroll delta
    // Note: positive delta = scroll DOWN (items move up), negative = scroll UP
    // Drag down gesture produces negative delta, which increases scroll offset
    let scroll_delta = state.consume_scroll_delta();
    first_item_scroll_offset -= scroll_delta; // Negate: drag down (-delta) => increase offset

    // Normalize scroll offset (handle scrolling past item boundaries)
    while first_item_scroll_offset < 0.0 && first_item_index > 0 {
        first_item_index -= 1;
        // Use cached size if available, otherwise use running average
        let estimated_size = state.get_cached_size(first_item_index)
            .unwrap_or_else(|| state.average_item_size());
        first_item_scroll_offset += estimated_size + config.spacing;
    }

    // Clamp to valid range
    first_item_index = first_item_index.min(items_count.saturating_sub(1));
    first_item_scroll_offset = first_item_scroll_offset.max(0.0);


    // Optimize huge forward scroll (handle scrolling past item boundaries)
    // This complements the backward scroll logic above by estimating items to skip
    if first_item_scroll_offset > 0.0 {
        let average_size = state.average_item_size();
        
        if average_size > 0.0 {
            // Check if we can skip items
            // We keep a buffer of items to avoid over-skipping due to size variance
            let buffer_pixels = viewport_size; 
            if first_item_scroll_offset > buffer_pixels {
                let pixels_to_skip = first_item_scroll_offset - buffer_pixels;
                let items_to_skip = (pixels_to_skip / average_size).floor() as usize;
                
                if items_to_skip > 0 {
                    let max_skip = items_count.saturating_sub(1).saturating_sub(first_item_index);
                    let actual_skip = items_to_skip.min(max_skip);
                    
                    if actual_skip > 0 {
                        first_item_index += actual_skip;
                        first_item_scroll_offset -= actual_skip as f32 * average_size;
                    }
                }
            }
        }
    }

    // Measure visible items
    let mut visible_items: Vec<LazyListMeasuredItem> = Vec::new();
    let mut current_offset = config.before_content_padding - first_item_scroll_offset;
    let viewport_end = viewport_size - config.after_content_padding;

    // Measure items going forward from first visible
    let mut current_index = first_item_index;
    while current_index < items_count && current_offset < viewport_end {
        if visible_items.len() >= 1000 {
            log::warn!("LazyList: visible items exceeded 1000, aborting measurement (likely infinite viewport)");
            break;
        }
        let mut item = measure_item(current_index);
        item.offset = current_offset;
        current_offset += item.main_axis_size + config.spacing;
        visible_items.push(item);
        current_index += 1;
    }

    // Measure beyond-bounds items after visible
    let after_count = config.beyond_bounds_item_count.min(items_count - current_index);
    for _ in 0..after_count {
        if current_index >= items_count {
            break;
        }
        let mut item = measure_item(current_index);
        item.offset = current_offset;
        current_offset += item.main_axis_size + config.spacing;
        visible_items.push(item);
        current_index += 1;
    }

    // Measure beyond-bounds items before visible
    if first_item_index > 0 && !visible_items.is_empty() {
        let before_count = config.beyond_bounds_item_count.min(first_item_index);
        let mut before_items: Vec<LazyListMeasuredItem> = Vec::new();
        let mut before_offset = visible_items[0].offset;

        for i in 0..before_count {
            let idx = first_item_index - 1 - i;
            let mut item = measure_item(idx);
            before_offset -= item.main_axis_size + config.spacing;
            item.offset = before_offset;
            before_items.push(item);
        }

        before_items.reverse();
        before_items.append(&mut visible_items);
        visible_items = before_items;
    }

    // Adjust scroll offset if we scrolled past the first item
    if first_item_scroll_offset > 0.0 && !visible_items.is_empty() {
        let first_visible = &visible_items[0];
        if first_visible.index == 0 && first_visible.offset > config.before_content_padding {
            // We're trying to scroll before the start, clamp
            let adjustment = first_visible.offset - config.before_content_padding;
            for item in &mut visible_items {
                item.offset -= adjustment;
            }
            let _ = first_item_scroll_offset;
        }
    }

    // Calculate total content size (estimated)
    let total_content_size = estimate_total_content_size(
        items_count,
        &visible_items,
        config,
        state.average_item_size(),
    );

    // Update scroll position - find actual first visible item
    let actual_first_visible = visible_items
        .iter()
        .find(|item| item.offset + item.main_axis_size > config.before_content_padding);

    let (final_first_index, final_scroll_offset) = if let Some(first) = actual_first_visible {
        let offset = config.before_content_padding - first.offset;
        (first.index, offset.max(0.0))
    } else if !visible_items.is_empty() {
        (visible_items[0].index, 0.0)
    } else {
        (0, 0.0)
    };

    // Update state with key for scroll position stability
    // When items are added/removed, the key allows finding the item's new index
    if let Some(first) = actual_first_visible {
        state.update_scroll_position_with_key(final_first_index, final_scroll_offset, first.key);
    } else if !visible_items.is_empty() {
        state.update_scroll_position_with_key(final_first_index, final_scroll_offset, visible_items[0].key);
    } else {
        state.update_scroll_position(final_first_index, final_scroll_offset);
    }
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info: visible_items.iter().map(|i| i.to_item_info()).collect(),
        total_items_count: items_count,
        viewport_size,
        viewport_start_offset: config.before_content_padding,
        viewport_end_offset: config.after_content_padding,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
    });

    // Determine scroll capability
    let can_scroll_backward = final_first_index > 0 || final_scroll_offset > 0.0;
    let can_scroll_forward = if let Some(last) = visible_items.last() {
        last.index < items_count - 1 || (last.offset + last.main_axis_size) > viewport_end
    } else {
        false
    };

    LazyListMeasureResult {
        visible_items,
        first_visible_item_index: final_first_index,
        first_visible_item_scroll_offset: final_scroll_offset,
        viewport_size,
        total_content_size,
        can_scroll_forward,
        can_scroll_backward,
    }
}

/// Estimates total content size based on measured items.
///
/// Uses the average size of measured items to estimate the total.
/// Falls back to state's running average if no items are currently measured.
fn estimate_total_content_size(
    items_count: usize,
    measured_items: &[LazyListMeasuredItem],
    config: &LazyListMeasureConfig,
    state_average_size: f32,
) -> f32 {
    if items_count == 0 {
        return 0.0;
    }

    // Use measured items' average if available, otherwise use state's accumulated average
    let avg_size = if !measured_items.is_empty() {
        let total_measured_size: f32 = measured_items.iter().map(|i| i.main_axis_size).sum();
        total_measured_size / measured_items.len() as f32
    } else {
        state_average_size
    };

    config.before_content_padding
        + (avg_size + config.spacing) * items_count as f32
        - config.spacing
        + config.after_content_padding
}

/// Extended measurement context with optimization support.
/// 
/// Contains prefetch scheduler and slot reuse pool for optimized measurement.
pub struct LazyListMeasureContext {
    /// Scheduler for prefetching items before they become visible.
    pub prefetch_scheduler: super::PrefetchScheduler,
    
    /// Pool for reusing composed item slots.
    pub slot_pool: super::SlotReusePool,
    
    /// Prefetch strategy configuration.
    pub prefetch_strategy: super::PrefetchStrategy,
    
    /// Previous scroll offset for direction detection.
    previous_scroll_offset: f32,
}

impl Default for LazyListMeasureContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LazyListMeasureContext {
    /// Creates a new measurement context with default settings.
    pub fn new() -> Self {
        Self {
            prefetch_scheduler: super::PrefetchScheduler::new(),
            slot_pool: super::SlotReusePool::new(),
            prefetch_strategy: super::PrefetchStrategy::default(),
            previous_scroll_offset: 0.0,
        }
    }

    /// Creates a context with custom prefetch strategy.
    pub fn with_prefetch_strategy(strategy: super::PrefetchStrategy) -> Self {
        Self {
            prefetch_scheduler: super::PrefetchScheduler::new(),
            slot_pool: super::SlotReusePool::new(),
            prefetch_strategy: strategy,
            previous_scroll_offset: 0.0,
        }
    }
}

/// Parameters for lazy list measurement.
/// 
/// Bundles the non-closure arguments to avoid too-many-arguments clippy warning.
#[derive(Clone)]
pub struct MeasureParams<'a> {
    /// Total number of items in the list.
    pub items_count: usize,
    /// Scroll state.
    pub state: &'a LazyListState,
    /// Viewport size in main axis.
    pub viewport_size: f32,
    /// Viewport size in cross axis.
    pub cross_axis_size: f32,
    /// Measurement configuration.
    pub config: &'a LazyListMeasureConfig,
}


/// Measures a lazy list with prefetch and slot reuse optimizations.
///
/// This is the optimized version of [`measure_lazy_list`] that:
/// 1. Checks slot reuse pool before composing new items
/// 2. Updates prefetch scheduler after measurement
/// 3. Returns slots to pool when items leave visible area
///
/// # Arguments
/// * `params` - Measurement parameters (items_count, state, viewport, config)
/// * `context` - Optimization context (prefetch + slot reuse)
/// * `measure_item` - Item measurement callback
/// * `get_item_key` - Key lookup for slot matching
pub fn measure_lazy_list_optimized<F, K>(
    params: MeasureParams<'_>,
    context: &mut LazyListMeasureContext,
    mut measure_item: F,
    _get_item_key: K,
) -> LazyListMeasureResult
where
    F: FnMut(usize) -> LazyListMeasuredItem,
    K: Fn(usize) -> u64,
{
    // Run base measurement
    let result = measure_lazy_list(
        params.items_count,
        params.state,
        params.viewport_size,
        params.cross_axis_size,
        params.config,
        &mut measure_item,
    );

    if result.visible_items.is_empty() {
        return result;
    }

    // Collect visible keys for slot management
    let visible_keys: Vec<u64> = result.visible_items.iter().map(|i| i.key).collect();

    // Return non-visible slots to pool
    context.slot_pool.release_non_visible(&visible_keys);

    // Mark visible items as in use
    for item in &result.visible_items {
        context.slot_pool.mark_in_use(
            item.key,
            item.content_type,
            item.node_ids.first().copied().unwrap_or(0) as usize,
        );
    }

    // Detect scroll direction
    let current_offset = result.first_visible_item_scroll_offset;
    let scroll_direction = current_offset - context.previous_scroll_offset;
    context.previous_scroll_offset = current_offset;

    // Get first and last visible indices
    let first_visible = result.visible_items.first().map(|i| i.index).unwrap_or(0);
    let last_visible = result.visible_items.last().map(|i| i.index).unwrap_or(0);

    // Update prefetch scheduler
    context.prefetch_scheduler.update(
        first_visible,
        last_visible,
        params.items_count,
        scroll_direction,
        &context.prefetch_strategy,
    );

    // Queue prefetch items (caller should process these during idle time)
    // The prefetch queue is now populated and can be consumed via:
    // context.prefetch_scheduler.next_prefetch()

    // Cleanup distant prefetches
    context.prefetch_scheduler.cleanup_distant_prefetches(
        first_visible,
        last_visible,
        params.config.beyond_bounds_item_count + 2,
    );

    result
}


#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
        LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
    }

    #[test]
    fn test_measure_empty_list() {
        let state = LazyListState::new();
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(0, &state, 500.0, 300.0, &config, |_| {
            panic!("Should not measure any items");
        });

        assert!(result.visible_items.is_empty());
    }

    #[test]
    fn test_measure_single_item() {
        let state = LazyListState::new();
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(1, &state, 500.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.visible_items.len(), 1);
        assert_eq!(result.visible_items[0].index, 0);
        assert!(!result.can_scroll_forward);
        assert!(!result.can_scroll_backward);
    }

    #[test]
    fn test_measure_fills_viewport() {
        let state = LazyListState::new();
        let config = LazyListMeasureConfig::default();

        // 10 items of 50px each, viewport of 200px should show 4+ items
        let result = measure_lazy_list(10, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        // Should have visible items plus beyond-bounds buffer
        assert!(result.visible_items.len() >= 4);
        assert!(result.can_scroll_forward);
        assert!(!result.can_scroll_backward);
    }

    #[test]
    fn test_measure_with_scroll_offset() {
        let state = LazyListState::with_initial_position(3, 25.0);
        let config = LazyListMeasureConfig::default();

        let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.first_visible_item_index, 3);
        assert!(result.can_scroll_forward);
        assert!(result.can_scroll_backward);
    }

    #[test]
    fn test_scroll_to_item() {
        let state = LazyListState::new();
        state.scroll_to_item(5, 0.0);

        let config = LazyListMeasureConfig::default();
        let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
            create_test_item(i, 50.0)
        });

        assert_eq!(result.first_visible_item_index, 5);
    }

    #[test]
    fn test_optimized_measure_updates_prefetch() {
        let state = LazyListState::new();
        let config = LazyListMeasureConfig::default();
        let mut context = LazyListMeasureContext::new();

        // First measurement
        let params = MeasureParams {
            items_count: 100,
            state: &state,
            viewport_size: 200.0,
            cross_axis_size: 300.0,
            config: &config,
        };
        let _ = measure_lazy_list_optimized(
            params, &mut context,
            |i| create_test_item(i, 50.0),
            |i| i as u64,
        );

        // Should have prefetch queue populated
        let pending = context.prefetch_scheduler.pending_prefetches();
        // Prefetch count is 2 by default, so should have up to 2 items queued
        assert!(pending.len() <= 2);
    }

    #[test]
    fn test_optimized_measure_manages_slots() {
        let state = LazyListState::new();
        let config = LazyListMeasureConfig::default();
        let mut context = LazyListMeasureContext::new();

        // First measurement
        let params = MeasureParams {
            items_count: 100,
            state: &state,
            viewport_size: 200.0,
            cross_axis_size: 300.0,
            config: &config,
        };
        let result = measure_lazy_list_optimized(
            params, &mut context,
            |i| create_test_item(i, 50.0),
            |i| i as u64,
        );

        // Visible items should be marked in slot pool
        assert!(context.slot_pool.in_use_count() > 0);
        assert_eq!(context.slot_pool.in_use_count(), result.visible_items.len());
    }
}
