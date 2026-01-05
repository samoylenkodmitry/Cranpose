//! Core measurement algorithm for lazy lists.
//!
//! This module implements the virtualized measurement logic that determines
//! which items should be composed and measured based on the current scroll
//! position and viewport size.

use super::bounds_adjuster::BoundsAdjuster;
use super::item_measurer::ItemMeasurer;
use super::lazy_list_measured_item::{LazyListMeasureResult, LazyListMeasuredItem};
use super::lazy_list_state::{LazyListLayoutInfo, LazyListState};
use super::scroll_position_resolver::ScrollPositionResolver;
use super::viewport::ViewportHandler;
use std::collections::VecDeque;

/// Default estimated item size for scroll calculations.
/// Used when no measured sizes are cached.
/// 48.0 is a common list item height (Material Design list tile).
pub const DEFAULT_ITEM_SIZE_ESTIMATE: f32 = 48.0;

/// Configuration for lazy list measurement.
#[derive(Clone, Debug)]
pub struct LazyListMeasureConfig {
    /// Whether the list is vertical (true) or horizontal (false).
    pub is_vertical: bool,

    /// Whether layout is reversed (items laid out from bottom/right to top/left).
    ///
    /// The measurement logic operates in a "start-to-end" coordinate system.
    /// This flag is used during placement to reverse the coordinates.
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

    /// Vertical arrangement for distributing items.
    /// Used when `is_vertical` is true.
    pub vertical_arrangement: Option<compose_ui_layout::LinearArrangement>,

    /// Horizontal arrangement for distributing items.
    /// Used when `is_vertical` is false.
    pub horizontal_arrangement: Option<compose_ui_layout::LinearArrangement>,
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
            vertical_arrangement: None,
            horizontal_arrangement: None,
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
    // reverse_layout is handled during placement (create_lazy_list_placements)
    // The measurement logic remains synonymous with "start" being the anchor edge

    // Handle empty list - reset scroll position to 0
    if items_count == 0 {
        state.update_scroll_position(0, 0.0);
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: Vec::new(),
            total_items_count: 0,
            viewport_size,
            viewport_start_offset: config.before_content_padding,
            viewport_end_offset: config.after_content_padding,
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
        });
        state.update_scroll_bounds();
        return LazyListMeasureResult::default();
    }

    // Handle zero/negative viewport - preserve existing scroll state
    // This can happen during collapsed states or measurement passes
    if viewport_size <= 0.0 {
        // Don't reset scroll position - just clear layout info
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: Vec::new(),
            total_items_count: items_count,
            viewport_size,
            viewport_start_offset: config.before_content_padding,
            viewport_end_offset: config.after_content_padding,
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
        });
        state.update_scroll_bounds();
        return LazyListMeasureResult::default();
    }

    // 1. Viewport handling - detect and handle infinite viewports
    let viewport = ViewportHandler::new(viewport_size, state.average_item_size(), config.spacing);
    let effective_viewport_size = viewport.effective_size();

    // 2. Resolve and normalize scroll position
    let resolver = ScrollPositionResolver::new(state, config, items_count, effective_viewport_size);
    let (mut first_index, mut first_offset) = resolver.apply_pending_scroll_delta();
    let mut pre_measured = Vec::new();

    // Backward scroll: use measured sizes to avoid sticky boundaries when estimates are wrong.
    if first_offset < 0.0 && first_index > 0 {
        (first_index, first_offset) = resolver.normalize_backward_jump(first_index, first_offset);
        while first_offset < 0.0 && first_index > 0 {
            first_index -= 1;
            let item = measure_item(first_index);
            first_offset += item.main_axis_size + config.spacing;
            pre_measured.push(item);
        }
        pre_measured.reverse();
    }

    first_index = first_index.min(items_count.saturating_sub(1));
    first_offset = first_offset.max(0.0);
    (first_index, first_offset) = resolver.normalize_forward(first_index, first_offset);

    // 3. Measure items (visible + beyond-bounds buffer)
    let pre_measured_queue = VecDeque::from(pre_measured);
    let mut measurer = ItemMeasurer::new(
        &mut measure_item,
        config,
        items_count,
        effective_viewport_size,
        pre_measured_queue,
    );
    let mut visible_items = measurer.measure_all(first_index, first_offset);

    // 4. Adjust bounds (clamp at start/end)
    let adjuster = BoundsAdjuster::new(config, items_count, effective_viewport_size);
    adjuster.clamp(&mut visible_items);

    // 5. Calculate total content size and finalize result
    let total_content_size = estimate_total_content_size(
        items_count,
        &visible_items,
        config,
        state.average_item_size(),
    );

    // Update scroll position - find actual first visible item
    let viewport_end = effective_viewport_size - config.after_content_padding;
    let item_end_with_spacing = |item: &LazyListMeasuredItem| {
        let spacing_after = if item.index + 1 < items_count {
            config.spacing
        } else {
            0.0
        };
        item.offset + item.main_axis_size + spacing_after
    };
    let actual_first_visible = visible_items
        .iter()
        .find(|item| item_end_with_spacing(item) > config.before_content_padding);

    let (final_first_index, final_scroll_offset) = if let Some(first) = actual_first_visible {
        let offset = config.before_content_padding - first.offset;
        (first.index, offset.max(0.0))
    } else if !visible_items.is_empty() {
        (visible_items[0].index, 0.0)
    } else {
        (0, 0.0)
    };

    // Update state with key for scroll position stability
    if let Some(first) = actual_first_visible {
        state.update_scroll_position_with_key(final_first_index, final_scroll_offset, first.key);
    } else if !visible_items.is_empty() {
        state.update_scroll_position_with_key(
            final_first_index,
            final_scroll_offset,
            visible_items[0].key,
        );
    } else {
        state.update_scroll_position(final_first_index, final_scroll_offset);
    }
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info: visible_items
            .iter()
            .filter(|item| {
                let item_end = item_end_with_spacing(item);
                item_end > config.before_content_padding && item.offset < viewport_end
            })
            .map(|i| i.to_item_info())
            .collect(),
        total_items_count: items_count,
        viewport_size: effective_viewport_size,
        viewport_start_offset: config.before_content_padding,
        viewport_end_offset: config.after_content_padding,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
    });

    // Update reactive scroll bounds from layout info
    state.update_scroll_bounds();

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
        viewport_size: effective_viewport_size,
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

    config.before_content_padding + (avg_size + config.spacing) * items_count as f32
        - config.spacing
        + config.after_content_padding
}

#[cfg(test)]
mod tests {
    use super::super::lazy_list_state::test_helpers::{
        new_lazy_list_state, new_lazy_list_state_with_position, with_test_runtime,
    };
    use super::*;

    fn create_test_item(index: usize, size: f32) -> LazyListMeasuredItem {
        LazyListMeasuredItem::new(index, index as u64, None, size, 100.0)
    }

    #[test]
    fn test_measure_empty_list() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig::default();

            let result = measure_lazy_list(0, &state, 500.0, 300.0, &config, |_| {
                panic!("Should not measure any items");
            });

            assert!(result.visible_items.is_empty());
        });
    }

    #[test]
    fn test_measure_single_item() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig::default();

            let result = measure_lazy_list(1, &state, 500.0, 300.0, &config, |i| {
                create_test_item(i, 50.0)
            });

            assert_eq!(result.visible_items.len(), 1);
            assert_eq!(result.visible_items[0].index, 0);
            assert!(!result.can_scroll_forward);
            assert!(!result.can_scroll_backward);
        });
    }

    #[test]
    fn test_measure_fills_viewport() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig::default();

            // 10 items of 50px each, viewport of 200px should show 4+ items
            let result = measure_lazy_list(10, &state, 200.0, 300.0, &config, |i| {
                create_test_item(i, 50.0)
            });

            // Should have visible items plus beyond-bounds buffer
            assert!(result.visible_items.len() >= 4);
            assert!(result.can_scroll_forward);
            assert!(!result.can_scroll_backward);
        });
    }

    #[test]
    fn test_measure_with_scroll_offset() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(3, 25.0);
            let config = LazyListMeasureConfig::default();

            let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
                create_test_item(i, 50.0)
            });

            assert_eq!(result.first_visible_item_index, 3);
            assert!(result.can_scroll_forward);
            assert!(result.can_scroll_backward);
        });
    }

    #[test]
    fn test_backward_scroll_uses_measured_size() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(1, 0.0);
            state.dispatch_scroll_delta(1.0);
            let config = LazyListMeasureConfig::default();

            let result = measure_lazy_list(2, &state, 100.0, 300.0, &config, |i| {
                if i == 0 {
                    create_test_item(i, 10.0)
                } else {
                    create_test_item(i, 100.0)
                }
            });

            assert_eq!(result.first_visible_item_index, 0);
            assert!((result.first_visible_item_scroll_offset - 9.0).abs() < 0.001);
        });
    }

    #[test]
    fn test_backward_scroll_with_spacing_preserves_offset_gap() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(1, 0.0);
            let config = LazyListMeasureConfig {
                spacing: 4.0,
                ..Default::default()
            };
            state.dispatch_scroll_delta(2.0);

            let result = measure_lazy_list(2, &state, 40.0, 300.0, &config, |i| {
                create_test_item(i, 50.0)
            });

            assert_eq!(result.first_visible_item_index, 0);
            assert!((result.first_visible_item_scroll_offset - 52.0).abs() < 0.001);
        });
    }

    #[test]
    fn test_scroll_to_item() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            state.scroll_to_item(5, 0.0);

            let config = LazyListMeasureConfig::default();
            let result = measure_lazy_list(20, &state, 200.0, 300.0, &config, |i| {
                create_test_item(i, 50.0)
            });

            assert_eq!(result.first_visible_item_index, 5);
        });
    }
}
