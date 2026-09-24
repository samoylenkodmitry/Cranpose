//! Core measurement algorithm for lazy lists.
//!
//! This module implements the virtualized measurement logic that determines
//! which items should be composed and measured based on the current scroll
//! position and viewport size.

use std::collections::VecDeque;

use super::{
    bounds_adjuster::BoundsAdjuster,
    diagnostics,
    item_measurer::{AlwaysMeasureBeyond, BeyondBoundsMeasurePolicy, ItemMeasurer},
    lazy_list_measured_item::{LazyListMeasureResult, LazyListMeasuredItem},
    lazy_list_state::{LazyListLayoutInfo, LazyListState},
    scroll_position_resolver::ScrollPositionResolver,
    viewport::ViewportHandler,
};

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

    if items_count == 0 {
        state.update_scroll_position(0, 0.0);
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: Vec::new(),
            total_items_count: 0,
            raw_viewport_size,
            is_infinite_viewport,
            viewport_size,
            viewport_start_offset: 0.0,
            viewport_end_offset: viewport_size,
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
            snap_anchor_offset: 0.0,
            reverse_layout: config.reverse_layout,
        });
        state.update_scroll_bounds();
        return LazyListMeasureResult::default();
    }

    if viewport_size <= 0.0 {
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info: Vec::new(),
            total_items_count: items_count,
            raw_viewport_size,
            is_infinite_viewport,
            viewport_size,
            viewport_start_offset: 0.0,
            viewport_end_offset: viewport_size.max(0.0),
            before_content_padding: config.before_content_padding,
            after_content_padding: config.after_content_padding,
            snap_anchor_offset: 0.0,
            reverse_layout: config.reverse_layout,
        });
        state.update_scroll_bounds();
        return LazyListMeasureResult::default();
    }

    let measure_state = state.begin_measure_pass();

    let viewport = ViewportHandler::new(
        viewport_size,
        measure_state.average_item_size,
        config.spacing,
    );
    if viewport.is_infinite() {
        return measure_unbounded_lazy_list(
            items_count,
            state,
            raw_viewport_size,
            config,
            &mut measure_item,
        );
    }
    let effective_viewport_size = viewport.effective_size();
    let is_infinite_viewport = viewport.is_infinite();

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

    if first_offset < 0.0 && first_index > 0 {
        (first_index, first_offset) = resolver.normalize_backward_jump(first_index, first_offset);
        while first_offset < 0.0 && first_index > 0 {
            first_index -= 1;
            let item = measure_item(first_index);
            first_offset += item.main_axis_size + config.spacing;
            if first_index == 0 {
                first_offset += config.before_content_padding;
            }
            pre_measured.push(item);
        }
        pre_measured.reverse();
    }

    first_index = first_index.min(items_count.saturating_sub(1));
    first_offset = first_offset.max(0.0);
    (first_index, first_offset) = resolver.normalize_forward_with_cache(first_index, first_offset);
    let item_extent_at = |index: usize, item_size: f32| {
        let leading_padding = if index == 0 {
            config.before_content_padding
        } else {
            0.0
        };
        let spacing_after = if index + 1 < items_count {
            config.spacing
        } else {
            0.0
        };
        leading_padding + item_size + spacing_after
    };
    let mut offset_known_within_current_item = state
        .get_cached_size(first_index)
        .is_some_and(|size| first_offset + 0.001 < item_extent_at(first_index, size));

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

    let adjuster = BoundsAdjuster::new(config, items_count, effective_viewport_size);
    adjuster.clamp(&mut visible_items);

    let total_content_size = estimate_total_content_size(
        items_count,
        &visible_items,
        config,
        measure_state.average_item_size,
    );

    let viewport_start = 0.0;
    let viewport_end = effective_viewport_size;
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
        .find(|item| item_end_with_spacing(item) > viewport_start);

    let unresolved_pass = measurement_hit_time_budget
        && !measurement_viewport_filled
        && actual_first_visible.is_none();

    let (final_first_index, final_scroll_offset) = if let Some(first) = actual_first_visible {
        let leading_padding = if first.index == 0 {
            config.before_content_padding
        } else {
            0.0
        };
        let offset = leading_padding - first.offset;
        (first.index, offset.max(0.0))
    } else if unresolved_pass {
        if pending_scroll_delta > 0.001 {
            let leading_padding = if measurement_start_index == 0 {
                config.before_content_padding
            } else {
                0.0
            };
            let preserved_offset = (leading_padding - measurement_start_offset).max(0.0);
            (measurement_start_index, preserved_offset)
        } else {
            let next_index = measurement_next_index.min(items_count.saturating_sub(1));
            if next_index + 1 >= items_count {
                (next_index, 0.0)
            } else {
                let leading_padding = if next_index == 0 {
                    config.before_content_padding
                } else {
                    0.0
                };
                let next_offset = (leading_padding - measurement_next_offset).max(0.0);
                (next_index, next_offset)
            }
        }
    } else if !visible_items.is_empty() {
        (visible_items[0].index, 0.0)
    } else {
        (0, 0.0)
    };

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
            if measurement_start_index == 0 {
                config.before_content_padding - measurement_start_offset
            } else {
                -measurement_start_offset
            },
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
                item_end > viewport_start && item.offset < viewport_end
            })
            .map(super::lazy_list_measured_item::LazyListMeasuredItem::to_item_info)
            .collect(),
        total_items_count: items_count,
        raw_viewport_size,
        is_infinite_viewport,
        viewport_size: effective_viewport_size,
        viewport_start_offset: viewport_start,
        viewport_end_offset: viewport_end,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
    });

    state.update_scroll_bounds();

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

/// Safety cap for the number of items realized when the viewport is
/// unbounded. Mirrors `MAX_VISIBLE_ITEMS_SAFETY` in the item measurer.
const MAX_UNBOUNDED_REALIZED_ITEMS: usize = 10_000;

/// Measures a lazy list whose main-axis viewport is unbounded/infinite.
///
/// Without a finite viewport there is nothing to virtualize against: every
/// item is realized sequentially from the start and the list reports its true
/// content extent. Scrolling is delegated entirely to the enclosing
/// scrollable, so the internal scroll position is pinned to the origin and
/// both scroll capabilities are reported as exhausted (which also stops the
/// scroll gesture detector from capturing drags that the outer scrollable
/// needs).
fn measure_unbounded_lazy_list<F>(
    items_count: usize,
    state: &LazyListState,
    raw_viewport_size: f32,
    config: &LazyListMeasureConfig,
    measure_item: &mut F,
) -> LazyListMeasureResult
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    let realized_count = items_count.min(MAX_UNBOUNDED_REALIZED_ITEMS);
    if realized_count < items_count {
        log::warn!(
            "LazyList: unbounded viewport with {items_count} items; realizing only the first {realized_count}. \
             Wrap the list in a constrained container to restore virtualization."
        );
    }

    let mut visible_items = Vec::with_capacity(realized_count);
    let mut offset = config.before_content_padding;
    for index in 0..realized_count {
        let mut item = measure_item(index);
        item.offset = offset;
        offset += item.main_axis_size;
        if index + 1 < items_count {
            offset += config.spacing;
        }
        visible_items.push(item);
    }
    let content_extent = offset + config.after_content_padding;

    state.update_scroll_position(0, 0.0);
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info: visible_items
            .iter()
            .map(super::lazy_list_measured_item::LazyListMeasuredItem::to_item_info)
            .collect(),
        total_items_count: items_count,
        raw_viewport_size,
        is_infinite_viewport: true,
        viewport_size: content_extent,
        viewport_start_offset: 0.0,
        viewport_end_offset: content_extent,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
    });
    state.update_scroll_bounds();

    let can_scroll_forward = realized_count < items_count;
    LazyListMeasureResult {
        visible_items,
        first_visible_item_index: 0,
        first_visible_item_scroll_offset: 0.0,
        viewport_size: content_extent,
        total_content_size: content_extent,
        can_scroll_forward,
        can_scroll_backward: false,
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
#[path = "tests/lazy_list_measure_tests.rs"]
mod tests;
