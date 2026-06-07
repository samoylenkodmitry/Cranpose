//! Core measurement algorithm for lazy lists.
//!
//! This module implements the virtualized measurement logic that determines
//! which items should be composed and measured based on the current scroll
//! position and viewport size.

use super::bounds_adjuster::BoundsAdjuster;
use super::diagnostics;
use super::item_measurer::{AlwaysMeasureBeyond, BeyondBoundsMeasurePolicy, ItemMeasurer};
use super::lazy_list_measured_item::{LazyListMeasureResult, LazyListMeasuredItem};
use super::lazy_list_state::{LazyListLayoutInfo, LazyListState};
use super::scroll_position_resolver::ScrollPositionResolver;
use super::viewport::ViewportHandler;
use std::collections::VecDeque;

/// Default estimated item size for scroll calculations.
/// Used when no measured sizes are cached.
/// 48.0 is a common list item height (Material Design list tile).
pub const DEFAULT_ITEM_SIZE_ESTIMATE: f32 = 48.0;
const MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS: usize = 8;
const MIN_ADAPTIVE_SCROLL_DELTA_ITEMS: f32 = 1.5;
const MIN_ACTIVE_SCROLL_WHEEL_BEYOND_BOUNDS_ITEMS: usize = 2;
const MIN_ACTIVE_SCROLL_FAST_BEYOND_BOUNDS_ITEMS: usize = MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS;
const MIN_IDLE_WARM_BEYOND_BOUNDS_ITEMS: usize = 4;

/// Configuration for lazy list measurement.
#[derive(Clone, Debug, PartialEq)]
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
    pub vertical_arrangement: Option<cranpose_ui_layout::LinearArrangement>,

    /// Horizontal arrangement for distributing items.
    /// Used when `is_vertical` is false.
    pub horizontal_arrangement: Option<cranpose_ui_layout::LinearArrangement>,
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
    measure_item: F,
) -> LazyListMeasureResult
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    measure_lazy_list_with_beyond_bounds_policy(
        items_count,
        state,
        viewport_size,
        _cross_axis_size,
        config,
        measure_item,
        AlwaysMeasureBeyond,
    )
}

pub fn measure_lazy_list_with_beyond_bounds_policy<F, B>(
    items_count: usize,
    state: &LazyListState,
    viewport_size: f32,
    _cross_axis_size: f32,
    config: &LazyListMeasureConfig,
    mut measure_item: F,
    beyond_bounds_policy: B,
) -> LazyListMeasureResult
where
    F: FnMut(usize) -> LazyListMeasuredItem,
    B: BeyondBoundsMeasurePolicy,
{
    let raw_viewport_size = viewport_size;
    let is_infinite_viewport = raw_viewport_size.is_infinite();

    // reverse_layout is handled during placement (create_lazy_list_placements)
    // The measurement logic remains synonymous with "start" being the anchor edge

    // Handle empty list - reset scroll position to 0
    if items_count == 0 {
        state.update_scroll_position(0, 0.0);
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: Vec::new(),
            total_items_count: 0,
            raw_viewport_size,
            is_infinite_viewport,
            viewport_size,
            viewport_start_offset: config.before_content_padding,
            viewport_end_offset: config.after_content_padding,
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
            snap_anchor_offset: 0.0,
            reverse_layout: config.reverse_layout,
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
            raw_viewport_size,
            is_infinite_viewport,
            viewport_size,
            viewport_start_offset: config.before_content_padding,
            viewport_end_offset: config.after_content_padding,
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
            snap_anchor_offset: 0.0,
            reverse_layout: config.reverse_layout,
        });
        state.update_scroll_bounds();
        return LazyListMeasureResult::default();
    }

    let measure_state = state.begin_measure_pass();

    // 1. Viewport handling - detect and handle infinite viewports
    let viewport = ViewportHandler::new(
        viewport_size,
        measure_state.average_item_size,
        config.spacing,
    );
    let effective_viewport_size = viewport.effective_size();
    let is_infinite_viewport = viewport.is_infinite();

    // 2. Resolve and normalize scroll position
    let pending_scroll_delta = measure_state.pending_scroll_delta;
    let resolver = ScrollPositionResolver::new(
        state,
        measure_state,
        config,
        items_count,
        effective_viewport_size,
    );
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
    (first_index, first_offset) = resolver.normalize_forward_with_cache(first_index, first_offset);
    let item_extent_at = |index: usize, item_size: f32| {
        let spacing_after = if index + 1 < items_count {
            config.spacing
        } else {
            0.0
        };
        item_size + spacing_after
    };
    let mut offset_known_within_current_item = state
        .get_cached_size(first_index)
        .map(|size| first_offset + 0.001 < item_extent_at(first_index, size))
        .unwrap_or(false);

    if !offset_known_within_current_item && first_offset > 0.0 && first_index < items_count {
        let item = measure_item(first_index);
        let item_extent = item_extent_at(first_index, item.main_axis_size);

        if first_offset + 0.001 < item_extent {
            pre_measured.push(item);
            offset_known_within_current_item = true;
        }
    }

    if !offset_known_within_current_item {
        (first_index, first_offset) = resolver.normalize_forward(first_index, first_offset);
    }

    // 3. Measure items (visible + beyond-bounds buffer)
    let pre_measured_queue = VecDeque::from(pre_measured);
    let telemetry_enabled = diagnostics::telemetry_enabled();
    let adaptive_beyond_bounds = adaptive_scroll_beyond_bounds_item_count(
        config,
        pending_scroll_delta,
        measure_state.average_item_size,
    );
    let guaranteed_beyond_bounds = adaptive_beyond_bounds;
    let mut measurer = ItemMeasurer::new(
        &mut measure_item,
        config,
        items_count,
        effective_viewport_size,
        measure_state.average_item_size,
        pre_measured_queue,
    )
    .with_beyond_bounds_item_count(adaptive_beyond_bounds)
    .with_guaranteed_after_beyond_bounds_item_count(guaranteed_beyond_bounds)
    .with_include_before_beyond_bounds(pending_scroll_delta >= -0.001)
    .with_beyond_bounds_measure_policy(beyond_bounds_policy)
    .with_telemetry_pass_id(telemetry_enabled.then(|| state.next_item_measure_pass_id()));
    let measurement_pass = measurer.measure_all(first_index, first_offset);
    let measurement_start_index = measurement_pass.start_index;
    let measurement_start_offset = measurement_pass.start_offset;
    let measurement_next_index = measurement_pass.next_index;
    let measurement_next_offset = measurement_pass.next_offset;
    let measurement_measured_visible_items = measurement_pass.measured_visible_items;
    let measurement_hit_time_budget = measurement_pass.hit_time_budget;
    let measurement_viewport_filled = measurement_pass.viewport_filled;
    let mut visible_items = measurement_pass.items;

    // 4. Adjust bounds (clamp at start/end)
    let adjuster = BoundsAdjuster::new(config, items_count, effective_viewport_size);
    adjuster.clamp(&mut visible_items);

    // 5. Calculate total content size and finalize result
    let total_content_size = estimate_total_content_size(
        items_count,
        &visible_items,
        config,
        measure_state.average_item_size,
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

    let unresolved_pass = measurement_hit_time_budget
        && !measurement_viewport_filled
        && actual_first_visible.is_none();

    let (final_first_index, final_scroll_offset) = if let Some(first) = actual_first_visible {
        let offset = config.before_content_padding - first.offset;
        (first.index, offset.max(0.0))
    } else if unresolved_pass {
        if pending_scroll_delta > 0.001 {
            let preserved_offset =
                (config.before_content_padding - measurement_start_offset).max(0.0);
            (measurement_start_index, preserved_offset)
        } else {
            let next_index = measurement_next_index.min(items_count.saturating_sub(1));
            if next_index + 1 >= items_count {
                (next_index, 0.0)
            } else {
                let next_offset =
                    (config.before_content_padding - measurement_next_offset).max(0.0);
                (next_index, next_offset)
            }
        }
    } else if !visible_items.is_empty() {
        (visible_items[0].index, 0.0)
    } else {
        (0, 0.0)
    };

    // Update state with key for scroll position stability
    if let Some(first) = actual_first_visible {
        state.update_scroll_position_with_key(final_first_index, final_scroll_offset, first.key);
    } else if !visible_items.is_empty() && !unresolved_pass {
        state.update_scroll_position_with_key(
            final_first_index,
            final_scroll_offset,
            visible_items[0].key,
        );
    } else {
        state.update_scroll_position(final_first_index, final_scroll_offset);
    }

    if telemetry_enabled {
        let cycle_id = state.next_measure_cycle_id();
        log::warn!(
            "[lazy-measure-telemetry] cycle={} items_count={} average_item_size={:.2} viewport_size={:.2} total_content_size={:.2} input_first_index={} input_first_offset={:.2} normalized_first_index={} normalized_first_offset={:.2} final_first_index={} final_first_offset={:.2} measured_visible={} total_measured={} unresolved_pass={} actual_first_visible={} timed_out={} viewport_filled={}",
            cycle_id,
            items_count,
            measure_state.average_item_size,
            effective_viewport_size,
            total_content_size,
            first_index,
            first_offset,
            measurement_start_index,
            config.before_content_padding - measurement_start_offset,
            final_first_index,
            final_scroll_offset,
            measurement_measured_visible_items,
            visible_items.len(),
            unresolved_pass,
            actual_first_visible.is_some(),
            measurement_hit_time_budget,
            measurement_viewport_filled
        );
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
        raw_viewport_size,
        is_infinite_viewport,
        viewport_size: effective_viewport_size,
        viewport_start_offset: config.before_content_padding,
        viewport_end_offset: config.after_content_padding,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
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

fn adaptive_scroll_beyond_bounds_item_count(
    config: &LazyListMeasureConfig,
    pending_scroll_delta: f32,
    average_item_size: f32,
) -> usize {
    let base_count = config.beyond_bounds_item_count;
    if pending_scroll_delta.abs() <= 0.001 {
        return if base_count == 0 {
            MIN_IDLE_WARM_BEYOND_BOUNDS_ITEMS
        } else {
            base_count
        };
    }
    let item_extent = if average_item_size.is_finite() && average_item_size > 0.0 {
        average_item_size
    } else {
        DEFAULT_ITEM_SIZE_ESTIMATE
    } + config.spacing.max(0.0);
    let item_extent = item_extent.max(1.0);
    let delta_items = pending_scroll_delta.abs() / item_extent;
    if delta_items < MIN_ADAPTIVE_SCROLL_DELTA_ITEMS {
        return base_count.max(MIN_ACTIVE_SCROLL_WHEEL_BEYOND_BOUNDS_ITEMS);
    }

    let adaptive_count = delta_items.ceil() as usize;
    base_count
        .max(MIN_ACTIVE_SCROLL_FAST_BEYOND_BOUNDS_ITEMS)
        .max(adaptive_count.min(MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS))
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
    fn lazy_list_measure_config_defaults_to_two_beyond_bounds_items() {
        let config = LazyListMeasureConfig::default();

        assert_eq!(config.beyond_bounds_item_count, 2);
    }

    #[test]
    fn active_scroll_guarantees_forward_warm_rows_for_single_wheel_ticks() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 0,
            spacing: 4.0,
            ..Default::default()
        };

        assert_eq!(
            adaptive_scroll_beyond_bounds_item_count(&config, -40.0, 48.0),
            2,
            "single wheel ticks should not force the full fast-scroll warm window"
        );
    }

    #[test]
    fn default_single_wheel_scroll_uses_configured_markdown_warm_window() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 2,
            spacing: 8.0,
            ..Default::default()
        };

        assert_eq!(
            adaptive_scroll_beyond_bounds_item_count(&config, -40.0, 120.0),
            2,
            "small Markdown wheel ticks must not measure eight cached text rows every frame"
        );
    }

    #[test]
    fn idle_measurement_warms_a_small_forward_window() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 0,
            spacing: 4.0,
            ..Default::default()
        };

        let adaptive = adaptive_scroll_beyond_bounds_item_count(&config, 0.0, 48.0);

        assert_eq!(adaptive, MIN_IDLE_WARM_BEYOND_BOUNDS_ITEMS);
    }

    #[test]
    fn active_scroll_guarantees_forward_warm_rows_when_configured_buffer_is_zero() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 0,
            spacing: 4.0,
            ..Default::default()
        };

        let adaptive = adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0);

        assert_eq!(adaptive, MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS);
    }

    #[test]
    fn adaptive_scroll_beyond_bounds_warms_fast_wheel_scroll_window() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 0,
            spacing: 4.0,
            ..Default::default()
        };

        assert_eq!(
            adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0),
            MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS
        );
    }

    #[test]
    fn adaptive_scroll_beyond_bounds_never_shrinks_configured_buffer() {
        let config = LazyListMeasureConfig {
            beyond_bounds_item_count: 12,
            spacing: 4.0,
            ..Default::default()
        };

        assert_eq!(
            adaptive_scroll_beyond_bounds_item_count(&config, -620.0, 48.0),
            12
        );
    }

    fn exact_scroll_position(
        item_sizes: &[f32],
        spacing: f32,
        viewport_size: f32,
        deltas: &[f32],
    ) -> Vec<(usize, f32)> {
        let total_content = item_sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                let spacing_after = if index + 1 < item_sizes.len() {
                    spacing
                } else {
                    0.0
                };
                size + spacing_after
            })
            .sum::<f32>();
        let max_scroll = (total_content - viewport_size).max(0.0);
        let mut scroll = 0.0f32;
        let mut positions = Vec::with_capacity(deltas.len());

        for delta in deltas {
            scroll = (scroll - delta).clamp(0.0, max_scroll);

            let mut remaining = scroll;
            let mut index = 0usize;
            while index + 1 < item_sizes.len() {
                let spacing_after = if index + 1 < item_sizes.len() {
                    spacing
                } else {
                    0.0
                };
                let extent = item_sizes[index] + spacing_after;
                if remaining < extent {
                    break;
                }
                remaining -= extent;
                index += 1;
            }
            positions.push((index, remaining));
        }

        positions
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

    #[test]
    fn test_time_budget_fills_visible_viewport_and_keeps_configured_beyond_bounds() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(100, 5_000.0);
            let config = LazyListMeasureConfig::default();

            let result = measure_lazy_list(10_000, &state, 100.0, 300.0, &config, |i| {
                std::thread::sleep(std::time::Duration::from_millis(8));
                create_test_item(i, 10.0)
            });

            assert_eq!(
                result.first_visible_item_index, 212,
                "time-budgeted pass should report the first item that actually reaches the viewport"
            );
            assert!(
                (result.first_visible_item_scroll_offset - 4.0).abs() < 1.0,
                "expected actual visible offset to be preserved"
            );
            assert_eq!(
                result.visible_items.first().map(|item| item.index),
                Some(200),
                "measurement should keep the configured leading retained items"
            );
            assert_eq!(
                result.visible_items.last().map(|item| item.index),
                Some(224),
                "measurement should keep the configured trailing retained items"
            );
            assert!(
                result
                    .visible_items
                    .last()
                    .is_some_and(|item| item.offset + item.main_axis_size >= 100.0),
                "visible measurement must fill the viewport before honoring the time budget"
            );
        });
    }

    #[test]
    fn test_time_budgeted_reverse_scroll_does_not_backtrack() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig {
                spacing: 8.0,
                ..Default::default()
            };
            let item_sizes: Vec<f32> = (0..512usize)
                .map(|index| match index % 7 {
                    0 => 44.0,
                    1 => 60.0,
                    2 => 220.0,
                    3 => 72.0,
                    4 => 96.0,
                    5 => 156.0,
                    _ => 52.0,
                })
                .collect();

            let mut result =
                measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                    std::thread::sleep(std::time::Duration::from_millis(55));
                    create_test_item(index, item_sizes[index])
                });
            assert_eq!(result.first_visible_item_index, 0);

            for _ in 0..4 {
                state.dispatch_scroll_delta(-320.0);
                result =
                    measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                        std::thread::sleep(std::time::Duration::from_millis(55));
                        create_test_item(index, item_sizes[index])
                    });
            }

            assert!(
                result.first_visible_item_index > 0,
                "expected to advance after forward time-budgeted scrolls"
            );

            let mut last_index = result.first_visible_item_index;
            for step in 0..4 {
                state.dispatch_scroll_delta(80.0);
                result =
                    measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |index| {
                        std::thread::sleep(std::time::Duration::from_millis(55));
                        create_test_item(index, item_sizes[index])
                    });
                assert!(
                    result.first_visible_item_index <= last_index,
                    "reverse time-budgeted step {step} backtracked from index {last_index} to {}",
                    result.first_visible_item_index
                );
                last_index = result.first_visible_item_index;
            }
        });
    }

    #[test]
    fn test_backward_scroll_does_not_advance_first_visible_index_for_variable_items() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig {
                spacing: 8.0,
                ..Default::default()
            };
            let item_sizes = [48.0, 56.0, 64.0, 72.0, 80.0];
            let measure_item =
                |index: usize| create_test_item(index, item_sizes[index % item_sizes.len()]);

            let mut result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
            assert_eq!(result.first_visible_item_index, 0);

            for _ in 0..28 {
                state.dispatch_scroll_delta(-32.0);
                result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
            }

            assert!(
                result.first_visible_item_index >= 12,
                "expected to scroll well into the list before reversing, got index={}",
                result.first_visible_item_index
            );

            let mut last_index = result.first_visible_item_index;
            for step in 0..24 {
                state.dispatch_scroll_delta(12.0);
                result = measure_lazy_list(200, &state, 260.0, 300.0, &config, measure_item);
                assert!(
                    result.first_visible_item_index <= last_index,
                    "backward step {step} advanced from index {last_index} to {}",
                    result.first_visible_item_index
                );
                last_index = result.first_visible_item_index;
            }
        });
    }

    #[test]
    fn test_stored_offset_inside_tall_item_does_not_skip_forward_without_pending_scroll() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(0, 900.0);
            let config = LazyListMeasureConfig {
                spacing: 8.0,
                ..Default::default()
            };
            let item_sizes: Vec<f32> = (0..32usize)
                .map(|index| if index == 0 { 1_200.0 } else { 64.0 })
                .collect();

            let result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |i| {
                create_test_item(i, item_sizes[i])
            });

            assert_eq!(
                result.first_visible_item_index, 0,
                "stored in-item offset must not be turned into an average-size forward jump"
            );
            assert!(
                (result.first_visible_item_scroll_offset - 900.0).abs() < 0.01,
                "expected to preserve the stored in-item scroll offset"
            );
        });
    }

    #[test]
    fn test_large_offset_inside_cached_tall_item_does_not_skip_forward_without_forward_scroll() {
        with_test_runtime(|| {
            let state = new_lazy_list_state_with_position(20, 900.0);
            let config = LazyListMeasureConfig {
                spacing: 8.0,
                ..Default::default()
            };
            for index in 0..20 {
                state.cache_item_size(index, 60.0 + (index % 3) as f32 * 8.0);
            }
            state.cache_item_size(20, 1_200.0);

            let item_sizes: Vec<f32> = (0..64usize)
                .map(|index| {
                    if index == 20 {
                        1_200.0
                    } else {
                        60.0 + (index % 3) as f32 * 8.0
                    }
                })
                .collect();

            let result = measure_lazy_list(item_sizes.len(), &state, 260.0, 320.0, &config, |i| {
                create_test_item(i, item_sizes[i])
            });

            assert_eq!(
                result.first_visible_item_index, 20,
                "offset within a tall cached item must not be interpreted as skipping to later average-sized items"
            );
            assert!(
                (result.first_visible_item_scroll_offset - 900.0).abs() < 0.01,
                "expected to preserve in-item offset inside the tall cached item"
            );
        });
    }

    #[test]
    fn test_matches_exact_model_for_variable_item_reverse_scrolls() {
        with_test_runtime(|| {
            let state = new_lazy_list_state();
            let config = LazyListMeasureConfig {
                spacing: 8.0,
                ..Default::default()
            };
            let viewport_size = 260.0;
            let item_sizes: Vec<f32> = (0..240usize)
                .map(|index| match index % 9 {
                    0 => 32.0,
                    1 => 48.0,
                    2 => 240.0,
                    3 => 56.0,
                    4 => 72.0,
                    5 => 180.0,
                    6 => 40.0,
                    7 => 96.0,
                    _ => 56.0,
                })
                .collect();
            let deltas = [
                -180.0, -180.0, -220.0, -150.0, -240.0, -120.0, -160.0, 60.0, 60.0, 80.0, -96.0,
                -96.0, 44.0, 44.0, 44.0, -140.0, -140.0, 72.0, 72.0, 72.0, 72.0,
            ];
            let expected =
                exact_scroll_position(&item_sizes, config.spacing, viewport_size, &deltas);

            for (step, (delta, (expected_index, expected_offset))) in
                deltas.iter().zip(expected.iter()).enumerate()
            {
                state.dispatch_scroll_delta(*delta);
                let mut result;
                loop {
                    result = measure_lazy_list(
                        item_sizes.len(),
                        &state,
                        viewport_size,
                        320.0,
                        &config,
                        |index| create_test_item(index, item_sizes[index]),
                    );
                    if state.peek_scroll_delta().abs() <= 0.001 {
                        break;
                    }
                }

                assert_eq!(
                    result.first_visible_item_index, *expected_index,
                    "step {step} delta={delta} expected first index {} but got {}",
                    expected_index, result.first_visible_item_index
                );
                assert!(
                    (result.first_visible_item_scroll_offset - *expected_offset).abs() < 0.01,
                    "step {step} delta={delta} expected offset {:.2} but got {:.2}",
                    expected_offset,
                    result.first_visible_item_scroll_offset
                );
            }
        });
    }
}
