//! Core measurement algorithm for lazy lists.
//!
//! This module implements the virtualized measurement logic that determines
//! which items should be composed and measured based on the current scroll
//! position and viewport size.

use std::{collections::VecDeque, convert::Infallible};

use cranpose_ui_layout::Axis;

use super::{
    bounds_adjuster::BoundsAdjuster,
    diagnostics,
    item_measurer::{ItemMeasurePass, ItemMeasurer},
    lazy_list_measured_item::{LazyListMeasureResult, LazyListMeasuredItem},
    lazy_list_state::{LazyListLayoutInfo, LazyListMeasureStateSnapshot, LazyListState},
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
    /// The axis along which the list lays out its items.
    pub axis: Axis,
    /// Whether horizontal placement resolves start from the right edge.
    pub is_rtl: bool,

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
    /// Used when `axis` is [`Axis::Vertical`].
    pub vertical_arrangement: Option<cranpose_ui_layout::LinearArrangement>,

    /// Horizontal arrangement for distributing items.
    /// Used when `axis` is [`Axis::Horizontal`].
    pub horizontal_arrangement: Option<cranpose_ui_layout::LinearArrangement>,
}

impl Default for LazyListMeasureConfig {
    fn default() -> Self {
        Self {
            axis: Axis::Vertical,
            is_rtl: false,
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

/// What a lazy list measure pass does with an item beyond its viewport.
pub enum BeyondItem {
    /// The item stays composed without being placed.
    Kept,
    /// The item is placed beside the viewport's items, where a focus search
    /// can move into it.
    Placed(LazyListMeasuredItem),
    /// The item is not kept, and neither is any further item on that side.
    Declined,
}

/// Where a lazy list measure pass gets its items.
pub trait LazyItemSource {
    /// Why the source gives no item: [`Infallible`] for a source that
    /// composes every item it is asked for, [`NeedsComposition`] for one
    /// that only keeps the items of the last pass.
    type Refusal;

    /// Composes and measures the item at `index`, which the pass places.
    fn measure(&mut self, index: usize) -> Result<LazyListMeasuredItem, Self::Refusal>;

    /// Keeps the item at `index`, which lies beyond the viewport, composed
    /// ahead of a scroll. `distance` says how far past the viewport's edge
    /// the item starts: measured for the items between, estimated from the
    /// average item size for the ones kept unplaced. A source places either
    /// every beyond item it keeps in a pass or none.
    fn keep_beyond(&mut self, index: usize, distance: f32) -> Result<BeyondItem, Self::Refusal>;
}

/// An item that a pass from the last pass's items cannot take as it is: it
/// needs a composition or a measure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedsComposition;

struct MeasureEveryItem<F>(F);

impl<F> LazyItemSource for MeasureEveryItem<F>
where
    F: FnMut(usize) -> LazyListMeasuredItem,
{
    type Refusal = Infallible;

    fn measure(&mut self, index: usize) -> Result<LazyListMeasuredItem, Infallible> {
        Ok((self.0)(index))
    }

    fn keep_beyond(&mut self, index: usize, _distance: f32) -> Result<BeyondItem, Infallible> {
        (self.0)(index);
        Ok(BeyondItem::Kept)
    }
}

/// Measures a lazy list and returns the items to compose/place.
///
/// This is the core algorithm that determines virtualization behavior:
/// 1. Handle pending scroll-to-item requests
/// 2. Apply scroll delta to current position
/// 3. Determine which items are visible in the viewport
/// 4. Compose and measure only those items, and keep the beyond-bounds
///    buffer composed without placing it
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
        &mut MeasureEveryItem(measure_item),
    )
}

/// [`measure_lazy_list`] with the items taken from `source`, which decides
/// how the items beyond the viewport are kept.
pub fn measure_lazy_list_with_beyond_bounds_policy<S>(
    items_count: usize,
    state: &LazyListState,
    viewport_size: f32,
    _cross_axis_size: f32,
    config: &LazyListMeasureConfig,
    source: &mut S,
) -> LazyListMeasureResult
where
    S: LazyItemSource<Refusal = Infallible>,
{
    if items_count == 0 || viewport_size <= 0.0 {
        return publish_without_items(state, items_count, viewport_size, config);
    }

    let measure_state = state.begin_measure_pass();

    let viewport = ViewportHandler::new(
        viewport_size,
        measure_state.average_item_size,
        config.spacing,
    );
    if viewport.is_infinite() {
        let Ok(result) =
            measure_unbounded_lazy_list(items_count, state, viewport_size, config, source);
        return result;
    }
    let Ok(list) = measure_items(
        items_count,
        state,
        (viewport_size, viewport.effective_size()),
        config,
        measure_state,
        source,
    );
    list.commit(state)
}

/// Measures a lazy list as [`measure_lazy_list_with_beyond_bounds_policy`]
/// does, from the items `source` keeps of its last pass, and leaves `state`
/// as it was: [`MeasuredLazyList::publish`] gives the state the pass. Refuses
/// where the source refuses an item, and a list with no items or with no
/// bounded viewport.
pub fn measure_lazy_list_from_retained<S>(
    items_count: usize,
    state: &LazyListState,
    viewport_size: f32,
    config: &LazyListMeasureConfig,
    source: &mut S,
) -> Result<MeasuredLazyList, NeedsComposition>
where
    S: LazyItemSource<Refusal = NeedsComposition>,
{
    if items_count == 0 || viewport_size <= 0.0 || ViewportHandler::is_unbounded(viewport_size) {
        return Err(NeedsComposition);
    }
    let measure_state = state.measure_pass_snapshot();
    let viewport = ViewportHandler::new(
        viewport_size,
        measure_state.average_item_size,
        config.spacing,
    );
    measure_items(
        items_count,
        state,
        (viewport_size, viewport.effective_size()),
        config,
        measure_state,
        source,
    )
}

/// A measured lazy list whose scroll position and layout info its state does
/// not hold yet.
#[must_use]
pub struct MeasuredLazyList {
    result: LazyListMeasureResult,
    // The key of the first visible item, which the scroll position follows
    // to where the item moves: none where the pass placed no item.
    first_key: Option<u64>,
    items_count: usize,
    raw_viewport_size: f32,
    padding: (f32, f32),
    spacing: f32,
    reverse_layout: bool,
    // The telemetry line of the pass, without its cycle number.
    telemetry: Option<String>,
}

impl MeasuredLazyList {
    /// Gives `state` the scroll position, the layout info and the scroll
    /// bounds of the pass, and takes the scroll the pass applied.
    pub fn publish(self, state: &LazyListState) -> LazyListMeasureResult {
        state.consume_measure_pass();
        self.commit(state)
    }

    fn commit(self, state: &LazyListState) -> LazyListMeasureResult {
        let result = self.result;
        let (first_index, first_offset) = (
            result.first_visible_item_index,
            result.first_visible_item_scroll_offset,
        );
        match self.first_key {
            Some(key) => state.update_scroll_position_with_key(first_index, first_offset, key),
            None => state.update_scroll_position(first_index, first_offset),
        }
        if let Some(line) = self.telemetry {
            let cycle_id = state.next_measure_cycle_id();
            log::warn!("[lazy-measure-telemetry] cycle={cycle_id} {line}");
        }
        let viewport_end = result.viewport_size;
        let mut visible_items_info = state.spare_items_info();
        visible_items_info.extend(
            result
                .visible_items
                .iter()
                .filter(|item| {
                    let item_end = item_end_with_spacing(item, self.items_count, self.spacing);
                    item_end > 0.0 && item.offset < viewport_end
                })
                .map(LazyListMeasuredItem::to_item_info),
        );
        state.update_layout_info(LazyListLayoutInfo {
            visible_items_info,
            total_items_count: self.items_count,
            raw_viewport_size: self.raw_viewport_size,
            is_infinite_viewport: false,
            viewport_size: result.viewport_size,
            viewport_start_offset: 0.0,
            viewport_end_offset: viewport_end,
            before_content_padding: self.padding.0,
            after_content_padding: self.padding.1,
            main_axis_item_spacing: self.spacing,
            snap_anchor_offset: 0.0,
            reverse_layout: self.reverse_layout,
        });

        state.update_scroll_bounds();
        result
    }
}

// Where an item ends, with the spacing after it unless it is the last.
fn item_end_with_spacing(item: &LazyListMeasuredItem, items_count: usize, spacing: f32) -> f32 {
    let spacing_after = if item.index + 1 < items_count {
        spacing
    } else {
        0.0
    };
    item.offset + item.main_axis_size + spacing_after
}

// Measures the items of a list with a bounded viewport, `viewport_sizes`
// as given and as the pass sees it, from `measure_state`.
fn measure_items<S>(
    items_count: usize,
    state: &LazyListState,
    viewport_sizes: (f32, f32),
    config: &LazyListMeasureConfig,
    measure_state: LazyListMeasureStateSnapshot,
    source: &mut S,
) -> Result<MeasuredLazyList, S::Refusal>
where
    S: LazyItemSource,
{
    let (raw_viewport_size, effective_viewport_size) = viewport_sizes;

    let pending_scroll_delta = measure_state.pending_scroll_delta;
    let window_scroll_delta = measure_state.window_scroll_delta;
    let resolver = ScrollPositionResolver::new(
        state,
        measure_state,
        config,
        items_count,
        effective_viewport_size,
    );
    let (first_index, first_offset, pre_measured) =
        resolve_start(&resolver, state, config, items_count, source)?;

    let telemetry_enabled = diagnostics::telemetry_enabled();
    let adaptive_beyond_bounds = adaptive_scroll_beyond_bounds_item_count(
        config,
        window_scroll_delta,
        measure_state.average_item_size,
    );
    let guaranteed_beyond_bounds = adaptive_beyond_bounds;
    let mut measurer = ItemMeasurer::new(
        source,
        config,
        items_count,
        effective_viewport_size,
        measure_state.average_item_size,
        VecDeque::from(pre_measured),
    )
    .with_beyond_bounds_item_count(adaptive_beyond_bounds)
    .with_guaranteed_after_beyond_bounds_item_count(guaranteed_beyond_bounds)
    .with_include_before_beyond_bounds(window_scroll_delta >= -0.001)
    .with_telemetry_pass_id(telemetry_enabled.then(|| state.next_item_measure_pass_id()));
    let mut measurement_pass = measurer.measure_all(first_index, first_offset)?;
    let mut visible_items = std::mem::take(&mut measurement_pass.items);

    let adjuster = BoundsAdjuster::new(config, items_count, effective_viewport_size);
    adjuster.clamp(&mut visible_items);

    let total_content_size = estimate_total_content_size(
        items_count,
        &visible_items,
        config,
        measure_state.average_item_size,
    );

    let position = final_position(
        &visible_items,
        &measurement_pass,
        config,
        items_count,
        pending_scroll_delta,
    );
    let telemetry = telemetry_enabled.then(|| {
        format!(
            "items_count={} average_item_size={:.2} viewport_size={:.2} total_content_size={:.2} input_first_index={} input_first_offset={:.2} normalized_first_index={} normalized_first_offset={:.2} final_first_index={} final_first_offset={:.2} measured_visible={} total_measured={} unresolved_pass={} actual_first_visible={} timed_out={} viewport_filled={}",
            items_count,
            measure_state.average_item_size,
            effective_viewport_size,
            total_content_size,
            first_index,
            first_offset,
            measurement_pass.start_index,
            if measurement_pass.start_index == 0 {
                config.before_content_padding - measurement_pass.start_offset
            } else {
                -measurement_pass.start_offset
            },
            position.index,
            position.offset,
            measurement_pass.measured_visible_items,
            visible_items.len(),
            position.unresolved,
            position.found,
            measurement_pass.hit_time_budget,
            measurement_pass.viewport_filled
        )
    });
    Ok(position.into_list(
        (visible_items, items_count),
        (raw_viewport_size, effective_viewport_size),
        total_content_size,
        config,
        telemetry,
    ))
}

// The content padding before the item at `index`: the list's for its first.
fn leading_padding(config: &LazyListMeasureConfig, index: usize) -> f32 {
    if index == 0 {
        config.before_content_padding
    } else {
        0.0
    }
}

// Where a pass starts measuring: the first item and how far it is scrolled
// once the pending scroll applies, with the items measured to find them.
fn resolve_start<S>(
    resolver: &ScrollPositionResolver<'_>,
    state: &LazyListState,
    config: &LazyListMeasureConfig,
    items_count: usize,
    source: &mut S,
) -> Result<(usize, f32, Vec<LazyListMeasuredItem>), S::Refusal>
where
    S: LazyItemSource,
{
    let (mut first_index, mut first_offset) = resolver.apply_pending_scroll_delta();
    let mut pre_measured = Vec::new();

    if first_offset < 0.0 && first_index > 0 {
        (first_index, first_offset) = resolver.normalize_backward_jump(first_index, first_offset);
        while first_offset < 0.0 && first_index > 0 {
            first_index -= 1;
            let item = source.measure(first_index)?;
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
        let spacing_after = if index + 1 < items_count {
            config.spacing
        } else {
            0.0
        };
        leading_padding(config, index) + item_size + spacing_after
    };
    let mut offset_known_within_current_item = state
        .get_cached_size(first_index)
        .is_some_and(|size| first_offset + 0.001 < item_extent_at(first_index, size));

    if !offset_known_within_current_item && first_offset > 0.0 && first_index < items_count {
        let item = source.measure(first_index)?;
        let item_extent = item_extent_at(first_index, item.main_axis_size);

        if first_offset + 0.001 < item_extent {
            pre_measured.push(item);
            offset_known_within_current_item = true;
        }
    }

    if !offset_known_within_current_item {
        (first_index, first_offset) = resolver.normalize_forward(first_index, first_offset);
    }
    Ok((first_index, first_offset, pre_measured))
}

// Where a pass leaves the list: its first visible item, how far it is
// scrolled and the key the position follows.
struct FinalPosition {
    index: usize,
    offset: f32,
    key: Option<u64>,
    // An item ends in the viewport.
    found: bool,
    // The pass ran out of time before it found one or filled the viewport.
    unresolved: bool,
}

impl FinalPosition {
    // The list of `items_count` items a pass with `visible_items` leaves at
    // this position.
    fn into_list(
        self,
        (visible_items, items_count): (Vec<LazyListMeasuredItem>, usize),
        (raw_viewport_size, viewport_size): (f32, f32),
        total_content_size: f32,
        config: &LazyListMeasureConfig,
        telemetry: Option<String>,
    ) -> MeasuredLazyList {
        let can_scroll_backward = self.index > 0 || self.offset > 0.0;
        let can_scroll_forward = visible_items.last().is_some_and(|last| {
            last.index < items_count - 1 || (last.offset + last.main_axis_size) > viewport_size
        });
        MeasuredLazyList {
            result: LazyListMeasureResult {
                visible_items,
                first_visible_item_index: self.index,
                first_visible_item_scroll_offset: self.offset,
                viewport_size,
                total_content_size,
                can_scroll_forward,
                can_scroll_backward,
            },
            first_key: self.key,
            items_count,
            raw_viewport_size,
            padding: (config.before_content_padding, config.after_content_padding),
            spacing: config.spacing,
            reverse_layout: config.reverse_layout,
            telemetry,
        }
    }
}

fn final_position(
    visible_items: &[LazyListMeasuredItem],
    pass: &ItemMeasurePass,
    config: &LazyListMeasureConfig,
    items_count: usize,
    pending_scroll_delta: f32,
) -> FinalPosition {
    let actual_first_visible = visible_items
        .iter()
        .find(|item| item_end_with_spacing(item, items_count, config.spacing) > 0.0);
    let unresolved =
        pass.hit_time_budget && !pass.viewport_filled && actual_first_visible.is_none();

    let (index, offset) = if let Some(first) = actual_first_visible {
        let offset = leading_padding(config, first.index) - first.offset;
        (first.index, offset.max(0.0))
    } else if unresolved {
        if pending_scroll_delta > 0.001 {
            let preserved_offset =
                (leading_padding(config, pass.start_index) - pass.start_offset).max(0.0);
            (pass.start_index, preserved_offset)
        } else {
            let next_index = pass.next_index.min(items_count.saturating_sub(1));
            if next_index + 1 >= items_count {
                (next_index, 0.0)
            } else {
                let next_offset = (leading_padding(config, next_index) - pass.next_offset).max(0.0);
                (next_index, next_offset)
            }
        }
    } else if !visible_items.is_empty() {
        (visible_items[0].index, 0.0)
    } else {
        (0, 0.0)
    };

    let key = match actual_first_visible {
        Some(first) => Some(first.key),
        None => visible_items
            .first()
            .filter(|_| !unresolved)
            .map(|item| item.key),
    };
    FinalPosition {
        index,
        offset,
        key,
        found: actual_first_visible.is_some(),
        unresolved,
    }
}

/// Publishes a pass that places no item, of a list with none or of a
/// viewport with no room. A list with no items rests at its start.
pub(super) fn publish_without_items(
    state: &LazyListState,
    items_count: usize,
    viewport_size: f32,
    config: &LazyListMeasureConfig,
) -> LazyListMeasureResult {
    if items_count == 0 {
        state.update_scroll_position(0, 0.0);
    }
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info: Vec::new(),
        total_items_count: items_count,
        raw_viewport_size: viewport_size,
        is_infinite_viewport: viewport_size.is_infinite(),
        viewport_size,
        viewport_start_offset: 0.0,
        viewport_end_offset: viewport_size.max(0.0),
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
        main_axis_item_spacing: config.spacing,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
    });
    state.update_scroll_bounds();
    LazyListMeasureResult::default()
}

/// Safety cap for the number of items realized when the viewport is
/// unbounded. Mirrors `MAX_VISIBLE_ITEMS_SAFETY` in the item measurer.
pub(super) const MAX_UNBOUNDED_REALIZED_ITEMS: usize = 10_000;

/// Measures a lazy list whose main-axis viewport is unbounded/infinite.
///
/// Without a finite viewport there is nothing to virtualize against: every
/// item is realized sequentially from the start and the list reports its true
/// content extent. Scrolling is delegated entirely to the enclosing
/// scrollable, so the internal scroll position is pinned to the origin and
/// both scroll capabilities are reported as exhausted (which also stops the
/// scroll gesture detector from capturing drags that the outer scrollable
/// needs).
fn measure_unbounded_lazy_list<S>(
    items_count: usize,
    state: &LazyListState,
    raw_viewport_size: f32,
    config: &LazyListMeasureConfig,
    source: &mut S,
) -> Result<LazyListMeasureResult, S::Refusal>
where
    S: LazyItemSource,
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
        let mut item = source.measure(index)?;
        item.offset = offset;
        offset += item.main_axis_size;
        if index + 1 < items_count {
            offset += config.spacing;
        }
        visible_items.push(item);
    }
    let content_extent = offset + config.after_content_padding;

    state.update_scroll_position(0, 0.0);
    let mut visible_items_info = state.spare_items_info();
    visible_items_info.extend(visible_items.iter().map(LazyListMeasuredItem::to_item_info));
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info,
        total_items_count: items_count,
        raw_viewport_size,
        is_infinite_viewport: true,
        viewport_size: content_extent,
        viewport_start_offset: 0.0,
        viewport_end_offset: content_extent,
        before_content_padding: config.before_content_padding,
        after_content_padding: config.after_content_padding,
        main_axis_item_spacing: config.spacing,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
    });
    state.update_scroll_bounds();

    let can_scroll_forward = realized_count < items_count;
    Ok(LazyListMeasureResult {
        visible_items,
        first_visible_item_index: 0,
        first_visible_item_scroll_offset: 0.0,
        viewport_size: content_extent,
        total_content_size: content_extent,
        can_scroll_forward,
        can_scroll_backward: false,
    })
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
        // Past the items a frame keeps ready, the window holds as many as the
        // scroll brings into view per frame, so a frame that finds enough
        // ready leaves the next ones to a pass between frames.
        return base_count.max(MIN_ACTIVE_SCROLL_WHEEL_BEYOND_BOUNDS_ITEMS)
            + delta_items.ceil() as usize;
    }

    let adaptive_count = delta_items.ceil() as usize;
    base_count
        .max(MIN_ACTIVE_SCROLL_FAST_BEYOND_BOUNDS_ITEMS)
        .max(adaptive_count.min(MAX_ADAPTIVE_SCROLL_BEYOND_BOUNDS_ITEMS))
}

#[cfg(test)]
#[path = "tests/lazy_list_measure_tests.rs"]
mod tests;
