//! LazyColumn and LazyRow widget implementations.
//!
//! Provides virtualized scrolling lists that only compose visible items,
//! matching Jetpack Compose's `LazyColumn` and `LazyRow` APIs.

#![expect(non_snake_case, reason = "names follow Compose's casing")]

use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{NodeId, SlotId, collections::map::HashMap};
use cranpose_foundation::lazy::{
    BeyondItem, LazyItemRoots, LazyItemSource, LazyListIntervalContent, LazyListMeasureConfig,
    LazyListMeasureResult, LazyListMeasuredItem, LazyListState, SmallNodeVec, SmallOffsetVec,
    measure_lazy_list_with_beyond_bounds_policy,
};
pub use cranpose_foundation::lazy::{LazyListItemInfo, LazyListLayoutInfo};
use cranpose_ui_layout::{
    Constraints, LinearArrangement, MeasureResult, MeasureScope as _, round_to_px,
};
use smallvec::SmallVec;
use web_time::Instant;

use super::PaddingValues;
use crate::{
    composable,
    layout::MeasuredNode,
    layout_direction::LayoutDirection,
    modifier::{Modifier, Size},
    scroll::{OverscrollEffect, ScrollMotionContextKey, scroll_motion_context_for_key},
    subcompose_layout::{
        MeasurePolicy, Placement, RetainedSlotChildren, SubcomposeChild, SubcomposeLayoutNode,
        SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
    },
};

const EXPENSIVE_RETAINED_REUSABLE_SLOTS: usize = 128;
const IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS: usize = 2;
const IDLE_PREFETCH_ITEMS_PER_PASS: usize = 1;
/// Where idle prefetch passes run, a scrolling frame composes the new items
/// beyond its viewport that its scroll would bring to the edge within this
/// many frames, and leaves the farther ones to the passes. A fast scroll
/// reaches the ready items, a slow one leaves them all.
const REACH_FRAMES: f32 = 2.0;

#[derive(Clone, Copy)]
struct LazyItemMeasureContext {
    index: usize,
    key_slot_id: u64,
    content_type: Option<u64>,
    is_vertical: bool,
    cross_axis_size: f32,
}

/// Specification for LazyColumn layout behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct LazyColumnSpec {
    /// Vertical arrangement for spacing between items.
    pub vertical_arrangement: LinearArrangement,
    /// Compose's `contentPadding`. The top and bottom edges add space before
    /// the first item and after the last; the start and end edges inset every
    /// item.
    pub content_padding: PaddingValues,
    /// Number of items to compose beyond the visible bounds.
    /// Higher values reduce jank during fast scrolling but use more memory.
    pub beyond_bounds_item_count: usize,
    /// Whether to reverse the layout direction (bottom-to-top).
    pub reverse_layout: bool,
}

impl Default for LazyColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: 2,
            reverse_layout: false,
        }
    }
}

impl LazyColumnSpec {
    /// Creates a spec with start arrangement, no content padding, two items beyond the bounds and a
    /// top-to-bottom layout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the vertical arrangement of the items, including the space between them.
    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    /// Pads the content on all four edges, as Compose's `contentPadding`.
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }

    /// The same content padding on all four edges.
    pub fn content_padding_all(self, padding: f32) -> Self {
        self.content_padding(PaddingValues::all(padding))
    }

    /// Sets whether the column lays out from bottom to top.
    pub fn reverse_layout(mut self, reverse: bool) -> Self {
        self.reverse_layout = reverse;
        self
    }
}

/// Specification for LazyRow layout behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct LazyRowSpec {
    /// Horizontal arrangement for spacing between items.
    pub horizontal_arrangement: LinearArrangement,
    /// Compose's `contentPadding`. The start and end edges add space before
    /// the first item and after the last; the top and bottom edges inset
    /// every item.
    pub content_padding: PaddingValues,
    /// Number of items to compose beyond the visible bounds.
    pub beyond_bounds_item_count: usize,
    /// Whether to reverse the layout direction (end-to-start).
    pub reverse_layout: bool,
}

impl Default for LazyRowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: 2,
            reverse_layout: false,
        }
    }
}

impl LazyRowSpec {
    /// Creates a spec with start arrangement, no content padding, two items beyond the bounds and a
    /// start-to-end layout.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the horizontal arrangement of the items, including the space between them.
    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    /// Pads the content on all four edges, as Compose's `contentPadding`.
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }

    /// The same content padding on all four edges.
    pub fn content_padding_all(self, padding: f32) -> Self {
        self.content_padding(PaddingValues::all(padding))
    }

    /// Sets whether the row lays out from end to start.
    pub fn reverse_layout(mut self, reverse: bool) -> Self {
        self.reverse_layout = reverse;
        self
    }
}

/// What a list measures and places with: the main-axis configuration and
/// the content padding on the physical cross-axis edges, left and right of a
/// column, top and bottom of a row.
#[derive(Clone, Debug, PartialEq)]
struct LazyListLayoutConfig {
    measure: LazyListMeasureConfig,
    cross_padding: (f32, f32),
    /// Lines of cells or lanes instead of single items, for a grid.
    grid: Option<grid::CellLayout>,
}

mod grid;
mod staggered;

pub use grid::{
    GridCells, LazyGridState, LazyHorizontalGrid, LazyHorizontalGridSpec, LazyVerticalGrid,
    LazyVerticalGridSpec, rememberLazyGridState,
};
pub use staggered::{
    LazyHorizontalStaggeredGrid, LazyHorizontalStaggeredGridSpec, LazyStaggeredGridState,
    LazyVerticalStaggeredGrid, LazyVerticalStaggeredGridSpec, rememberLazyStaggeredGridState,
};

/// A list's content padding along its axes, in layout pixels rounded to the
/// device grid as Compose's `roundToPx` rounds it.
struct ResolvedContentPadding {
    /// Before the first item and after the last, in layout order: a reversed
    /// column starts at its bottom edge, a reversed row at its end edge.
    before: f32,
    after: f32,
    /// The physical cross-axis edges: left and right of a column, top and
    /// bottom of a row.
    cross: (f32, f32),
    /// The physical main-axis edges: top and bottom of a column, left and
    /// right of a row.
    main: (f32, f32),
}

fn resolve_content_padding(
    padding: PaddingValues,
    is_vertical: bool,
    reverse_layout: bool,
    direction: LayoutDirection,
    density: f32,
) -> ResolvedContentPadding {
    let px = |value: f32| round_to_px(value.max(0.0), density);
    let edges = padding.physical(direction);
    let (left, top, right, bottom) = (
        px(edges.left),
        px(edges.top),
        px(edges.right),
        px(edges.bottom),
    );
    // A row lays out from its start edge; right-to-left placement mirrors it.
    let (first, last) = if is_vertical {
        (top, bottom)
    } else {
        (px(padding.start), px(padding.end))
    };
    let (before, after) = if reverse_layout {
        (last, first)
    } else {
        (first, last)
    };
    if is_vertical {
        ResolvedContentPadding {
            before,
            after,
            cross: (left, right),
            main: (top, bottom),
        }
    } else {
        ResolvedContentPadding {
            before,
            after,
            cross: (top, bottom),
            main: (left, right),
        }
    }
}

#[derive(Clone, Copy)]
struct LazyListItemMeasureInputs<'a> {
    is_vertical: bool,
    cross_axis_size: f32,
    content: &'a LazyListIntervalContent,
    state: &'a LazyListState,
    measured_item_cache: &'a Rc<RefCell<LazyMeasuredItemCache>>,
}

/// The measurements of all an item's roots, in the order of its roots: what
/// [`LazyItemRoots::measurements`] holds as `dyn Any`.
pub(crate) type RootMeasurements = SmallVec<[Rc<MeasuredNode>; 4]>;

/// The measurement of each of `roots`, when the item holds one for each.
pub(crate) fn root_measurements(roots: &LazyItemRoots) -> Option<&RootMeasurements> {
    roots
        .measurements
        .as_ref()?
        .downcast_ref::<RootMeasurements>()
}

fn measure_lazy_list_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    index: usize,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> LazyListMeasuredItem {
    let key = inputs.content.get_key(index);
    let key_slot_id = key.to_slot_id();
    let content_type = inputs.content.get_content_type(index);
    let slot_id = SlotId(key_slot_id);
    let item_context = LazyItemMeasureContext {
        index,
        key_slot_id,
        content_type,
        is_vertical: inputs.is_vertical,
        cross_axis_size: inputs.cross_axis_size,
    };

    scope.update_content_type(slot_id, content_type);

    // The cache stays borrowed while the slot activates: activating it runs
    // no composition and reads no lazy list cache, so the candidate found
    // here is still the cache's when the slot turns out clean.
    let candidate_cache = inputs.measured_item_cache.borrow();
    let candidate = candidate_cache.candidate(&item_context);
    let stale_candidate = candidate.is_none() && candidate_cache.contains_index(index);

    if let Some(cached) = candidate {
        match scope
            .activate_exact_retained_slot_with_known_children(slot_id, &cached.item.roots.node_ids)
        {
            Some(RetainedSlotChildren::Clean) => {
                let item = cached.item.clone();
                drop(candidate_cache);
                let mut cache = inputs.measured_item_cache.borrow_mut();
                cache.record_candidate_hit();
                cache.record_exact_reuse();
                return item;
            }
            Some(RetainedSlotChildren::Remeasure {
                children,
                children_match,
            }) => {
                drop(candidate_cache);
                let mut cache = inputs.measured_item_cache.borrow_mut();
                cache.record_candidate_hit();
                cache.remove(index);
                if children_match {
                    cache.record_dirty_children();
                } else {
                    cache.record_exact_miss();
                }
                drop(cache);
                return measure_lazy_list_children(
                    scope,
                    children,
                    inputs.measured_item_cache,
                    item_context,
                    Instant::now(),
                );
            }
            None => {
                drop(candidate_cache);
                let mut cache = inputs.measured_item_cache.borrow_mut();
                cache.record_candidate_hit();
                cache.record_exact_miss();
                cache.remove(index);
            }
        }
    } else {
        drop(candidate_cache);
        let mut cache = inputs.measured_item_cache.borrow_mut();
        cache.record_candidate_miss();
        if stale_candidate {
            cache.remove(index);
        }
    }

    // An item the cache keeps reads no clock: only composing and measuring
    // one costs time worth recording.
    let measure_start = Instant::now();
    let Some(root_children) = subcompose_lazy_list_item(scope, index, key, inputs) else {
        return LazyListMeasuredItem::new(
            index,
            key_slot_id,
            content_type,
            1.0,
            0.0,
            Rc::default(),
        );
    };

    let was_reused = scope.was_last_slot_reused().unwrap_or(false);
    inputs.state.record_composition(was_reused);

    let root_node_ids: SmallNodeVec = root_children
        .iter()
        .map(|child| child.node_id() as u64)
        .collect();

    let cache = inputs.measured_item_cache.borrow();
    let has_cached_item = cache.get(&item_context, &root_node_ids).is_some();
    let mut remove_cache_entry = !has_cached_item && cache.contains_index(index);
    drop(cache);

    if has_cached_item {
        if !scope.children_need_relayout(&root_children) {
            let cache = inputs.measured_item_cache.borrow();
            if let Some(cached) = cache.get(&item_context, &root_node_ids) {
                let item = cached.item.clone();
                drop(cache);
                return item;
            }
            drop(cache);
        }
        remove_cache_entry = true;
    }
    if remove_cache_entry {
        inputs.measured_item_cache.borrow_mut().remove(index);
    }

    let item = measure_lazy_list_children(
        scope,
        root_children,
        inputs.measured_item_cache,
        item_context,
        measure_start,
    );
    crate::lazy_prefetch::record_lazy_item_cost(measure_start.elapsed());
    item
}

fn subcompose_lazy_list_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    index: usize,
    key: cranpose_foundation::lazy::LazyLayoutKey,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> Option<Vec<SubcomposeChild>> {
    let key_slot_id = key.to_slot_id();
    let item_identity = key.is_user_key().then_some(key_slot_id);
    let item_content = inputs
        .content
        .with_interval(index, |local_index, interval| {
            let content = Rc::clone(&interval.content);
            move || {
                crate::lazy_item::ProvideLazyItemKey(item_identity, || (content)(local_index));
            }
        })?;
    Some(scope.subcompose(SlotId(key_slot_id), (), item_content))
}

fn keep_lazy_list_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    index: usize,
    inputs: &LazyListItemMeasureInputs<'_>,
) {
    let key = inputs.content.get_key(index);
    let slot_id = SlotId(key.to_slot_id());
    scope.update_content_type(slot_id, inputs.content.get_content_type(index));
    if !scope.keep_clean_retained_slot(slot_id, &()) {
        let _ = subcompose_lazy_list_item(scope, index, key, inputs);
    }
}

fn lazy_list_child_constraints(is_vertical: bool, cross_axis_size: f32) -> Constraints {
    if is_vertical {
        Constraints {
            min_width: 0.0,
            max_width: cross_axis_size,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    } else {
        Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: cross_axis_size,
        }
    }
}

fn measure_lazy_list_children(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    root_children: Vec<SubcomposeChild>,
    measured_item_cache: &Rc<RefCell<LazyMeasuredItemCache>>,
    context: LazyItemMeasureContext,
    measure_start: Instant,
) -> LazyListMeasuredItem {
    let child_constraints =
        lazy_list_child_constraints(context.is_vertical, context.cross_axis_size);

    let mut total_main_size: f32 = 0.0;
    let mut max_cross_size: f32 = 0.0;
    let mut node_ids: SmallNodeVec = SmallVec::with_capacity(root_children.len());
    let mut child_offsets: SmallOffsetVec = SmallVec::new();
    let mut child_cross_axis_sizes: SmallOffsetVec = SmallVec::new();
    let roots = root_children.len();
    let mut measurements = RootMeasurements::with_capacity(roots);

    for child in root_children {
        let (placeable, retained) = scope.measure_retained(child, child_constraints);
        let size = retained.as_ref().map_or_else(
            || Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            |measured| measured.size(),
        );
        let (main, cross) = if context.is_vertical {
            (size.height, size.width)
        } else {
            (size.width, size.height)
        };

        child_offsets.push(total_main_size);
        child_cross_axis_sizes.push(cross);
        node_ids.push(child.node_id() as u64);
        if let Some(retained) = retained {
            measurements.push(retained);
        }

        total_main_size += main;
        max_cross_size = max_cross_size.max(cross);
    }

    let main_axis_size = total_main_size.max(1.0);
    let item = LazyListMeasuredItem::new(
        context.index,
        context.key_slot_id,
        context.content_type,
        main_axis_size,
        max_cross_size,
        Rc::new(LazyItemRoots {
            node_ids,
            child_offsets,
            child_cross_axis_sizes,
            measurements: (measurements.len() == roots)
                .then(|| Rc::new(measurements) as Rc<dyn Any>),
        }),
    );

    measured_item_cache
        .borrow_mut()
        .insert(item.clone(), context.cross_axis_size);
    let elapsed = measure_start.elapsed();
    if cranpose_core::env_flag!("CRANPOSE_LAZY_ITEM_TELEMETRY") {
        log::warn!(
            "[lazy-item-telemetry] index={} children={} main={:.2} cross={:.2} elapsed_ms={:.2}",
            context.index,
            item.roots.node_ids.len(),
            item.main_axis_size,
            item.cross_axis_size,
            elapsed.as_secs_f64() * 1000.0
        );
    }
    measured_item_cache.borrow_mut().record_uncached_measure();
    item
}

/// Recycles the active slots of items before `first_measured_index`, found
/// among the items `near` the viewport, where every slot a forward scroll
/// skips was active.
fn recycle_forward_skipped_active_slots(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    near: &std::ops::Range<usize>,
    first_measured_index: usize,
    scroll_delta: f32,
) -> bool {
    if scroll_delta >= -0.001 || first_measured_index == 0 {
        return false;
    }

    scope.recycle_active_slots_where(|slot_id| {
        content
            .get_index_by_slot_id_in_range(slot_id.raw(), near.clone())
            .is_some_and(|index| index < first_measured_index)
    });
    true
}

/// Internal helper to create a lazy list measure policy.
struct LazyListMeasureContext<'a> {
    state: &'a LazyListState,
    config: &'a LazyListLayoutConfig,
    measured_item_cache: &'a Rc<RefCell<LazyMeasuredItemCache>>,
    overscroll: &'a OverscrollEffect,
}

/// The constraint-derived sizes a measure pass works with.
struct LazyFrame {
    is_vertical: bool,
    raw_viewport_size: f32,
    cross_axis_size: f32,
    /// The cross-axis size inside the cross-axis content padding.
    item_cross_axis_size: f32,
}

impl LazyFrame {
    fn new(constraints: Constraints, is_vertical: bool, config: &LazyListLayoutConfig) -> Self {
        let (raw_viewport_size, cross_axis_size) = if is_vertical {
            (constraints.max_height, constraints.max_width)
        } else {
            (constraints.max_width, constraints.max_height)
        };
        let (cross_start, cross_end) = config.cross_padding;
        Self {
            is_vertical,
            raw_viewport_size,
            cross_axis_size,
            item_cross_axis_size: (cross_axis_size - cross_start - cross_end).max(0.0),
        }
    }
}

fn measure_lazy_list_internal(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    is_vertical: bool,
    content: &Rc<LazyListIntervalContent>,
    context: LazyListMeasureContext<'_>,
) -> MeasureResult {
    let frame = LazyFrame::new(constraints, is_vertical, context.config);
    let retained_reusable_slots = EXPENSIVE_RETAINED_REUSABLE_SLOTS;
    scope.set_reusable_pool_limits(retained_reusable_slots, retained_reusable_slots);
    let (result, count) = match &context.config.grid {
        Some(grid::CellLayout::Grid(grid)) => {
            grid::measure_grid_viewport(scope, content, &context, &frame, grid)
        }
        Some(grid::CellLayout::Staggered(layout)) => {
            staggered::measure_staggered_viewport(scope, content, &context, &frame, layout)
        }
        None => (
            measure_list_viewport(scope, content, &context, &frame),
            content.item_count(),
        ),
    };
    log_lazy_cache_telemetry(&result, context.measured_item_cache);
    place_lazy_list(scope, constraints, &result, count, &context, &frame)
}

/// Measures the items of a list's viewport, places the focused one and the
/// sticky header, and registers their measurements.
fn measure_list_viewport(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    context: &LazyListMeasureContext<'_>,
    frame: &LazyFrame,
) -> LazyListMeasureResult {
    let state = context.state;
    let config = &context.config.measure;
    let measured_item_cache = context.measured_item_cache;
    let items_count = content.item_count();
    measured_item_cache.borrow_mut().begin_pass(
        scope,
        frame.is_vertical,
        frame.item_cross_axis_size,
    );
    let near = state.nearest_range();
    if items_count > 0 {
        state.update_scroll_position_if_item_moved(items_count, |slot_id| {
            content.get_index_by_slot_id_in_range(slot_id, near.clone())
        });
    }

    let scroll_delta_for_direction = state.peek_scroll_delta();
    let item_measure_inputs = LazyListItemMeasureInputs {
        is_vertical: frame.is_vertical,
        cross_axis_size: frame.item_cross_axis_size,
        content,
        state,
        measured_item_cache,
    };

    let focused_item = measure_focused_lazy_item(scope, &item_measure_inputs);
    let node_id = scope.root_id();
    let place_beyond = scope.focused_slot().is_some();

    let mut result = measure_lazy_viewport(
        LazyListItems {
            scope: &mut *scope,
            inputs: item_measure_inputs,
            near,
            scroll_delta: scroll_delta_for_direction,
            skipped_slots_recycled: false,
            place_beyond,
            beyond: None,
        },
        config,
        frame.raw_viewport_size,
        node_id,
    );
    if let Some(item) = focused_item {
        let cache = measured_item_cache.borrow();
        let known = cache.entries.iter().filter_map(|(&index, cached)| {
            (cached.item.key == content.get_key(index).to_slot_id())
                .then_some((index, cached.item.main_axis_size))
        });
        place_focused_lazy_item(&mut result, item, known, config.spacing);
    }
    pin_sticky_header(&mut result, content.header_indices(), |index| {
        measure_lazy_list_item(scope, index, &item_measure_inputs)
    });
    result
}

/// Sizes the list from what it measured and places the items of `result`,
/// `count` lines of them.
fn place_lazy_list(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    result: &LazyListMeasureResult,
    count: usize,
    context: &LazyListMeasureContext<'_>,
    frame: &LazyFrame,
) -> MeasureResult {
    let state = context.state;
    let overscroll = context.overscroll;
    let is_vertical = frame.is_vertical;
    let cross_axis_size = frame.cross_axis_size;
    let effective_viewport_size = result.viewport_size;
    overscroll.set_dimension(effective_viewport_size);

    state.cache_item_sizes(
        result
            .visible_items
            .iter()
            .map(|item| (item.index, item.main_axis_size)),
    );
    let truly_visible_count = result
        .visible_items
        .iter()
        .filter(|item| {
            let item_end = item.offset + item.main_axis_size;
            item.offset < effective_viewport_size && item_end > 0.0
        })
        .count();
    let in_pool = scope.reusable_slots_count();
    state.update_stats(truly_visible_count, in_pool);

    let resolve_main_axis = |content_size: f32, min: f32, max: f32| {
        if max.is_finite() {
            content_size.clamp(min, max)
        } else {
            content_size.min(effective_viewport_size).max(min)
        }
    };

    let width = if is_vertical {
        cross_axis_size
    } else {
        resolve_main_axis(
            result.total_content_size,
            constraints.min_width,
            constraints.max_width,
        )
    };
    let height = if is_vertical {
        resolve_main_axis(
            result.total_content_size,
            constraints.min_height,
            constraints.max_height,
        )
    } else {
        cross_axis_size
    };

    let density = scope.density();
    scope.layout_with_placed_roots(width, height, |placements, placed_roots| {
        push_lazy_list_placements(
            placements,
            density,
            &result.visible_items,
            count,
            is_vertical,
            effective_viewport_size,
            context.config,
            cross_axis_size,
        );
        // The placements follow the items and their roots in this order.
        placed_roots.extend(
            result
                .visible_items
                .iter()
                .map(|item| Rc::clone(&item.roots)),
        );
        let bounce = overscroll.offset();
        for placement in placements.iter_mut() {
            if is_vertical {
                placement.y += bounce;
            } else {
                placement.x += bounce;
            }
        }
    })
}

struct LazyListItems<'s, 'a, 'i> {
    scope: &'s mut SubcomposeMeasureScopeImpl<'a>,
    inputs: LazyListItemMeasureInputs<'i>,
    near: std::ops::Range<usize>,
    scroll_delta: f32,
    skipped_slots_recycled: bool,
    place_beyond: bool,
    beyond: Option<BeyondBoundsComposition>,
}

impl LazyListItems<'_, '_, '_> {
    fn composed(&self, index: usize) -> bool {
        self.scope
            .slot_is_active(SlotId(self.inputs.content.get_key(index).to_slot_id()))
    }

    /// Whether the pass keeps an item `distance` past the viewport.
    fn admits(&mut self, composed: bool, distance: f32) -> bool {
        self.beyond
            .as_mut()
            .is_none_or(|beyond| beyond.admits(composed, distance))
    }

    /// Keeps the item at `index` composed without placing it.
    fn keep(&mut self, index: usize, composed: bool) {
        if composed {
            keep_lazy_list_item(self.scope, index, &self.inputs);
        } else {
            self.measure(index);
        }
    }
}

impl LazyItemSource for LazyListItems<'_, '_, '_> {
    fn measure(&mut self, index: usize) -> LazyListMeasuredItem {
        if !self.skipped_slots_recycled
            && recycle_forward_skipped_active_slots(
                self.scope,
                self.inputs.content,
                &self.near,
                index,
                self.scroll_delta,
            )
        {
            self.skipped_slots_recycled = true;
        }
        // An item an idle pass composes afresh has its modifier slices built
        // before the frame it enters the screen in.
        let fresh =
            self.beyond.as_ref().is_some_and(|beyond| beyond.idle_pass) && !self.composed(index);
        let item = measure_lazy_list_item(self.scope, index, &self.inputs);
        if fresh {
            crate::lazy_prefetch::note_prefetched_item(&item.roots.node_ids);
        }
        item
    }

    fn keep_beyond(&mut self, index: usize, distance: f32) -> BeyondItem {
        let composed = self.composed(index);
        if !self.admits(composed, distance) {
            return BeyondItem::Declined;
        }
        if self.place_beyond {
            return BeyondItem::Placed(self.measure(index));
        }
        self.keep(index, composed);
        BeyondItem::Kept
    }
}

/// How this pass composes the items beyond the viewport. While the list
/// scrolls, and in an idle prefetch pass, they compose as
/// [`BeyondBoundsComposition`] decides; so do they at rest where the shell
/// runs idle prefetch passes, unless the list holds the focus and places them.
fn beyond_composition(items: &LazyListItems<'_, '_, '_>) -> Option<BeyondBoundsComposition> {
    if crate::lazy_prefetch::in_lazy_prefetch_pass() {
        items.inputs.state.hold_scroll_window();
        return Some(BeyondBoundsComposition::new(true, f32::INFINITY));
    }
    let scrolling = items.scroll_delta.abs() > 0.001;
    let passes_run = crate::lazy_prefetch::idle_prefetch_passes_run();
    (scrolling || (passes_run && !items.place_beyond)).then(|| {
        let reach = if passes_run {
            items.scroll_delta.abs() * REACH_FRAMES
        } else {
            f32::INFINITY
        };
        BeyondBoundsComposition::new(false, reach)
    })
}

/// Asks for a pass for the list `node_id` when this one left an item beyond
/// the viewport for later.
fn request_beyond_items(beyond: Option<BeyondBoundsComposition>, node_id: NodeId) {
    if beyond.is_some_and(|beyond| beyond.wants_prefetch) {
        crate::lazy_prefetch::request_lazy_prefetch(node_id);
    }
}

/// Measures the list's viewport, composing the items beyond it as
/// [`beyond_composition`] decides.
fn measure_lazy_viewport(
    mut items: LazyListItems<'_, '_, '_>,
    config: &LazyListMeasureConfig,
    viewport_size: f32,
    node_id: NodeId,
) -> LazyListMeasureResult {
    items.beyond = beyond_composition(&items);
    let inputs = items.inputs;
    let result = measure_lazy_list_with_beyond_bounds_policy(
        inputs.content.item_count(),
        inputs.state,
        viewport_size,
        inputs.cross_axis_size,
        config,
        &mut items,
    );
    request_beyond_items(items.beyond, node_id);
    result
}

/// Which items beyond the viewport a list composes, in order away from it.
/// Composed items stay composed. A frame composes new ones that start within
/// its reach until enough composed items lie ahead, counting its own; the
/// next is left for an idle prefetch pass, which composes one.
#[derive(Clone, Copy)]
struct BeyondBoundsComposition {
    idle_pass: bool,
    ready: usize,
    frontier: usize,
    /// How far past the viewport a new item may start for a frame to compose
    /// it. Where idle prefetch passes run, the distance its scroll covers in
    /// [`REACH_FRAMES`] frames, none at rest: the passes compose the rest.
    reach: f32,
    wants_prefetch: bool,
}

impl BeyondBoundsComposition {
    fn new(idle_pass: bool, reach: f32) -> Self {
        Self {
            idle_pass,
            ready: 0,
            frontier: if idle_pass {
                IDLE_PREFETCH_ITEMS_PER_PASS
            } else {
                IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS
            },
            reach,
            wants_prefetch: false,
        }
    }

    fn admits(&mut self, composed: bool, distance: f32) -> bool {
        if composed {
            self.ready += 1;
            return true;
        }
        let deferred = !self.idle_pass
            && (self.ready >= IDLE_PREFETCH_READY_BEYOND_BOUNDS_ITEMS || distance >= self.reach);
        if deferred || self.frontier == 0 {
            self.wants_prefetch = true;
            return false;
        }
        self.frontier -= 1;
        self.ready += 1;
        true
    }
}

fn measure_focused_lazy_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> Option<LazyListMeasuredItem> {
    let index = focused_lazy_item(scope, inputs)?;
    Some(measure_lazy_list_item(scope, index, inputs))
}

/// The index of the item that holds the focus. Focus in an item the
/// content no longer has is cleared.
fn focused_lazy_item(
    scope: &SubcomposeMeasureScopeImpl<'_>,
    inputs: &LazyListItemMeasureInputs<'_>,
) -> Option<usize> {
    let slot = scope.focused_slot()?.raw();
    let content = inputs.content;
    let last_measured = inputs.measured_item_cache.borrow().focused_index(slot);
    let found = last_measured
        .filter(|&index| {
            index < content.item_count() && content.get_key(index).to_slot_id() == slot
        })
        .or_else(|| content.get_index_by_slot_id_in_range(slot, inputs.state.nearest_range()))
        .or_else(|| content.get_index_by_slot_id(slot));
    let Some(index) = found else {
        let _ = cranpose_core::run_in_mutable_snapshot(|| crate::FocusManager.clear_focus());
        return None;
    };
    inputs
        .measured_item_cache
        .borrow_mut()
        .remember_focused(slot, index);
    Some(index)
}

/// Places the focused `item` where the pass left it out, as far from the
/// first visible item as the items between take: `known` sizes, the first
/// visible item's size for the others.
fn place_focused_lazy_item(
    result: &mut LazyListMeasureResult,
    mut item: LazyListMeasuredItem,
    known: impl IntoIterator<Item = (usize, f32)>,
    spacing: f32,
) {
    let Err(position) = result
        .visible_items
        .binary_search_by_key(&item.index, |entry| entry.index)
    else {
        return;
    };
    let Some(anchor) = result.visible_items.first() else {
        return;
    };
    let start = item.index.min(anchor.index);
    let end = item.index.max(anchor.index);
    let estimate = anchor.main_axis_size;
    let mut distance = (end - start) as f32 * (estimate + spacing);
    for (index, size) in known {
        if (start..end).contains(&index) {
            distance += size - estimate;
        }
    }
    item.offset = anchor.offset
        + if item.index < anchor.index {
            -distance
        } else {
            distance
        };
    result.visible_items.insert(position, item);
}

/// Holds the sticky header of the first visible item at the start of the
/// viewport, composing it once it scrolled out, until the next header
/// pushes it out. The header goes last, so it draws over the items that
/// scroll under it and takes the touches there. A list whose items all fit
/// keeps its headers where they are.
fn pin_sticky_header(
    result: &mut LazyListMeasureResult,
    headers: &[usize],
    measure: impl FnOnce(usize) -> LazyListMeasuredItem,
) {
    if headers.is_empty() || !(result.can_scroll_backward || result.can_scroll_forward) {
        return;
    }
    let Some(first_visible) = result
        .visible_items
        .iter()
        .find(|item| item.offset + item.main_axis_size > 0.0)
        .map(|item| item.index)
    else {
        return;
    };
    let following = headers.partition_point(|&header| header <= first_visible);
    let Some(&current) = following.checked_sub(1).and_then(|at| headers.get(at)) else {
        return;
    };
    let next_offset = headers.get(following).and_then(|&next| {
        result
            .visible_items
            .iter()
            .find(|item| item.index == next)
            .map(|item| item.offset)
    });
    let mut header = match result
        .visible_items
        .iter()
        .position(|item| item.index == current)
    {
        Some(at) => result.visible_items.remove(at),
        None => {
            let mut header = measure(current);
            header.offset = 0.0;
            header
        }
    };
    header.offset = header.offset.max(0.0);
    if let Some(next) = next_offset {
        header.offset = header.offset.min(next - header.main_axis_size);
    }
    result.visible_items.push(header);
}

fn bind_layout_invalidation_callback(
    state: LazyListState,
    overscroll: OverscrollEffect,
    list_state_id: usize,
    node_id: NodeId,
) {
    let callback_owner =
        cranpose_core::remember(|| Rc::new(RefCell::new(None::<u64>))).with(Clone::clone);
    let app_context_id = crate::render_state::current_app_context_id();
    let callback_id = state.try_register_layout_callback(
        node_id,
        Rc::new(move || {
            let _ = crate::render_state::enter_app_context_by_id(app_context_id, || {
                crate::schedule_measure_repass(node_id);
            });
        }),
    );

    if let Some(previous_id) = callback_owner.replace(callback_id)
        && Some(previous_id) != callback_id
    {
        state.remove_invalidate_callback(previous_id);
    }

    let overscroll_callback_owner =
        cranpose_core::remember(|| Rc::new(RefCell::new(None::<u64>))).with(Clone::clone);
    let overscroll_callback_id = overscroll.add_invalidate_callback(Box::new(move || {
        let _ = crate::render_state::enter_app_context_by_id(app_context_id, || {
            crate::schedule_measure_repass(node_id);
            crate::schedule_semantics_invalidation(node_id);
        });
    }));
    if let Some(previous_id) = overscroll_callback_owner.replace(Some(overscroll_callback_id)) {
        overscroll.remove_invalidate_callback(previous_id);
    }

    cranpose_core::DisposableEffect(
        (list_state_id, node_id, callback_id, overscroll_callback_id),
        move |scope| {
            scope.on_dispose(move || {
                if let Some(callback_id) = callback_id {
                    state.remove_invalidate_callback(callback_id);
                }
                overscroll.remove_invalidate_callback(overscroll_callback_id);
            })
        },
    );
}

#[derive(Clone)]
struct LazyListContentHandle(Rc<LazyListIntervalContent>);

impl LazyListContentHandle {
    /// The content that `add` adds its items to.
    fn build(add: impl FnOnce(&mut LazyListIntervalContent)) -> Self {
        let mut content = LazyListIntervalContent::new();
        add(&mut content);
        Self(Rc::new(content))
    }

    fn empty() -> Self {
        Self::build(|_| {})
    }

    fn content(&self) -> &Rc<LazyListIntervalContent> {
        &self.0
    }
}

impl PartialEq for LazyListContentHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Default)]
struct LazyMeasuredItemCache {
    is_vertical: bool,
    cross_axis_bits: u32,
    telemetry: LazyCacheTelemetry,
    entries: HashMap<usize, CachedLazyMeasuredItem>,
    focused: Option<(u64, usize)>,
}

struct CachedLazyMeasuredItem {
    item: LazyListMeasuredItem,
    /// The cross size the item was measured at. A grid cell's changes with
    /// its span, its column and the grid's width.
    cross_axis_bits: u32,
}

#[derive(Clone, Copy, Default)]
struct LazyCacheTelemetry {
    candidate_hits: usize,
    candidate_misses: usize,
    exact_reuses: usize,
    exact_misses: usize,
    dirty_children: usize,
    uncached_measures: usize,
}

impl LazyCacheTelemetry {
    fn has_events(self) -> bool {
        self.candidate_hits > 0
            || self.candidate_misses > 0
            || self.exact_reuses > 0
            || self.exact_misses > 0
            || self.dirty_children > 0
            || self.uncached_measures > 0
    }
}

impl LazyMeasuredItemCache {
    /// The index the item in `slot` was last measured at while focused, the
    /// first place to look for it: a focused item scrolled far from the
    /// viewport is found there without scanning the list.
    fn focused_index(&self, slot: u64) -> Option<usize> {
        self.focused
            .filter(|(focused, _)| *focused == slot)
            .map(|(_, index)| index)
    }

    fn remember_focused(&mut self, slot: u64, index: usize) {
        self.focused = Some((slot, index));
    }

    fn retain_constraint_scope(&mut self, is_vertical: bool, cross_axis_size: f32) {
        let cross_axis_bits = normalized_axis_bits(cross_axis_size);
        if self.entries.is_empty() {
            self.is_vertical = is_vertical;
            self.cross_axis_bits = cross_axis_bits;
            return;
        }
        if self.is_vertical != is_vertical || self.cross_axis_bits != cross_axis_bits {
            self.clear();
            self.is_vertical = is_vertical;
            self.cross_axis_bits = cross_axis_bits;
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drops what a new measure pass cannot reuse: everything, when the
    /// list's axis or cross size changed, and otherwise the items whose slots
    /// `scope` no longer holds.
    fn begin_pass(
        &mut self,
        scope: &SubcomposeMeasureScopeImpl<'_>,
        is_vertical: bool,
        cross_axis_size: f32,
    ) {
        self.retain_constraint_scope(is_vertical, cross_axis_size);
        self.retain_retained_slots(|slot_id| scope.slot_is_retained(slot_id));
    }

    /// Keeps only the items whose slots the subcompose state still holds.
    /// An item whose composition was disposed can never be reused, and its
    /// entry would keep the item's whole measured tree alive: an endless list
    /// scrolled past thousands of items kept them all.
    fn retain_retained_slots(&mut self, is_retained: impl Fn(SlotId) -> bool) {
        self.entries
            .retain(|_, cached| is_retained(SlotId(cached.item.key)));
    }

    fn get(
        &self,
        item: &LazyItemMeasureContext,
        node_ids: &SmallNodeVec,
    ) -> Option<&CachedLazyMeasuredItem> {
        self.candidate(item)
            .filter(|cached| cached.item.roots.node_ids == *node_ids)
    }

    fn contains_index(&self, index: usize) -> bool {
        self.entries.contains_key(&index)
    }

    fn candidate(&self, item: &LazyItemMeasureContext) -> Option<&CachedLazyMeasuredItem> {
        let cached = self.entries.get(&item.index)?;
        if cached.item.key != item.key_slot_id
            || cached.item.content_type != item.content_type
            || cached.cross_axis_bits != normalized_axis_bits(item.cross_axis_size)
            || cached.item.roots.measurements.is_none()
        {
            return None;
        }
        Some(cached)
    }

    fn remove(&mut self, index: usize) {
        self.entries.remove(&index);
    }

    fn insert(&mut self, item: LazyListMeasuredItem, cross_axis_size: f32) {
        let index = item.index;
        let cached = CachedLazyMeasuredItem {
            item,
            cross_axis_bits: normalized_axis_bits(cross_axis_size),
        };
        self.entries.insert(index, cached);
    }

    fn record_candidate_hit(&mut self) {
        self.telemetry.candidate_hits += 1;
    }

    fn record_candidate_miss(&mut self) {
        self.telemetry.candidate_misses += 1;
    }

    fn record_exact_reuse(&mut self) {
        self.telemetry.exact_reuses += 1;
    }

    fn record_exact_miss(&mut self) {
        self.telemetry.exact_misses += 1;
    }

    fn record_dirty_children(&mut self) {
        self.telemetry.dirty_children += 1;
    }

    fn take_telemetry(&mut self) -> LazyCacheTelemetry {
        std::mem::take(&mut self.telemetry)
    }

    fn record_uncached_measure(&mut self) {
        self.telemetry.uncached_measures += 1;
    }
}

fn log_lazy_cache_telemetry(
    result: &LazyListMeasureResult,
    measured_item_cache: &Rc<RefCell<LazyMeasuredItemCache>>,
) {
    if !cranpose_core::env_flag!("CRANPOSE_LAZY_CACHE_TELEMETRY") {
        return;
    }

    let telemetry = measured_item_cache.borrow_mut().take_telemetry();
    if !telemetry.has_events() {
        return;
    }

    let message = format!(
        "[lazy-cache-telemetry] first={} offset={:.2} visible={} candidate_hits={} candidate_misses={} exact_reuses={} exact_misses={} dirty_children={} uncached_measures={} cache_entries={}",
        result.first_visible_item_index,
        result.first_visible_item_scroll_offset,
        result.visible_items.len(),
        telemetry.candidate_hits,
        telemetry.candidate_misses,
        telemetry.exact_reuses,
        telemetry.exact_misses,
        telemetry.dirty_children,
        telemetry.uncached_measures,
        measured_item_cache.borrow().entries.len(),
    );
    log::warn!("{message}");
}

fn normalized_axis_bits(size: f32) -> u32 {
    if size.is_finite() && size >= 0.0 {
        size.to_bits()
    } else {
        f32::INFINITY.to_bits()
    }
}

#[expect(clippy::too_many_arguments)]
fn push_lazy_list_placements(
    placements: &mut Vec<Placement>,
    density: f32,
    visible_items: &[LazyListMeasuredItem],
    items_count: usize,
    is_vertical: bool,
    viewport_size: f32,
    layout_config: &LazyListLayoutConfig,
    cross_axis_size: f32,
) {
    use cranpose_ui_layout::Arrangement;

    let config = &layout_config.measure;

    placements.clear();
    placements.reserve(
        visible_items
            .iter()
            .map(|item| item.roots.node_ids.len())
            .sum(),
    );

    let arrangement = if is_vertical {
        config
            .vertical_arrangement
            .unwrap_or(LinearArrangement::Start)
    } else {
        config
            .horizontal_arrangement
            .unwrap_or(LinearArrangement::Start)
    };

    let spacing = config.spacing;
    let total_item_size: f32 = visible_items.iter().map(|i| i.main_axis_size).sum::<f32>()
        + (items_count.saturating_sub(1) as f32) * spacing;
    let available_main_axis =
        (viewport_size - config.before_content_padding - config.after_content_padding).max(0.0);
    let has_spare_space =
        total_item_size < available_main_axis && visible_items.len() == items_count;
    let should_apply_arrangement = has_spare_space
        && !matches!(
            arrangement,
            LinearArrangement::Start | LinearArrangement::SpacedBy(_)
        );

    if should_apply_arrangement {
        let content_offset = config.before_content_padding;

        let sizes: SmallVec<[f32; 32]> = visible_items.iter().map(|i| i.main_axis_size).collect();
        let mut positions: SmallVec<[f32; 32]> = SmallVec::from_elem(0.0, sizes.len());
        arrangement.arrange(density, available_main_axis, &sizes, &mut positions);

        for (item, &pos) in visible_items.iter().zip(positions.iter()) {
            push_lazy_item_placements(
                placements,
                item,
                content_offset + pos,
                is_vertical,
                viewport_size,
                cross_axis_size,
                layout_config,
            );
        }
    } else {
        for item in visible_items {
            push_lazy_item_placements(
                placements,
                item,
                item.offset,
                is_vertical,
                viewport_size,
                cross_axis_size,
                layout_config,
            );
        }
    }
}

fn push_lazy_item_placements(
    placements: &mut Vec<Placement>,
    item: &LazyListMeasuredItem,
    offset: f32,
    is_vertical: bool,
    viewport_size: f32,
    cross_axis_size: f32,
    layout_config: &LazyListLayoutConfig,
) {
    let config = &layout_config.measure;
    let (cross_start, cross_end) = layout_config.cross_padding;
    let start = if config.reverse_layout {
        viewport_size - offset - item.main_axis_size
    } else {
        offset
    };
    for (index, (&node_id, &offset)) in item
        .roots
        .node_ids
        .iter()
        .zip(&item.roots.child_offsets)
        .enumerate()
    {
        let main = start + offset;
        let cell = item.child_cross_offsets.get(index).copied().unwrap_or(0.0);
        let (mut x, y) = if is_vertical {
            (cross_start + cell, main)
        } else {
            (main, cross_start + cell)
        };
        if config.is_rtl {
            x = if is_vertical {
                cross_axis_size
                    - cross_end
                    - cell
                    - item
                        .roots
                        .child_cross_axis_sizes
                        .get(index)
                        .copied()
                        .unwrap_or(item.cross_axis_size)
            } else {
                let extent = item
                    .child_main_sizes
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| {
                        item.roots
                            .child_offsets
                            .get(index + 1)
                            .copied()
                            .unwrap_or(item.main_axis_size)
                            - offset
                    });
                viewport_size - x - extent
            };
        }
        placements.push(Placement::new(node_id as NodeId, x, y, 0));
    }
}

fn lazy_list_state_identity(state: &LazyListState) -> usize {
    let state_ptr = state.inner_ptr();
    debug_assert!(
        !state_ptr.is_null(),
        "lazy list identity requires a live LazyListState"
    );
    state_ptr as usize
}

fn lazy_list_state_only_recomposition(state: &LazyListState) -> bool {
    cranpose_core::current_recompose_scope_invalidated_only_by(state.reactive_state_ids())
        .unwrap_or(false)
}

/// Internal implementation for LazyColumn that takes pre-built content.
///
/// Users should prefer the DSL-based [`LazyColumn`] function instead.
fn LazyColumnImpl(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyColumnSpec,
    grid: Option<grid::CellLayout>,
    content: LazyListContentHandle,
) -> NodeId {
    use std::cell::RefCell;

    let content_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyListContentHandle::empty())))
            .with(Clone::clone);

    let refresh_content = !lazy_list_state_only_recomposition(&state);
    if refresh_content {
        *content_cell.borrow_mut() = content;
    }
    let caller_modifier_changed = super::layout::caller_modifier_changed(&modifier);

    let composed_density = crate::density::density();
    let direction = crate::layout_direction();
    let padding = resolve_content_padding(
        spec.content_padding,
        true,
        spec.reverse_layout,
        direction,
        composed_density.density(),
    );
    let semantics_grid = grid.clone();
    let config = LazyListLayoutConfig {
        measure: LazyListMeasureConfig {
            is_vertical: true,
            is_rtl: direction.is_rtl(),
            reverse_layout: spec.reverse_layout,
            before_content_padding: padding.before,
            after_content_padding: padding.after,
            spacing: spec
                .vertical_arrangement
                .spacing(composed_density.density()),
            beyond_bounds_item_count: spec.beyond_bounds_item_count,
            vertical_arrangement: Some(spec.vertical_arrangement),
            horizontal_arrangement: None,
        },
        cross_padding: padding.cross,
        grid,
    };
    let config_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(config.clone()))).with(Clone::clone);
    let config_changed = {
        let mut current = config_cell.borrow_mut();
        let changed = *current != config;
        if changed {
            *current = config;
        }
        changed
    };
    let measured_item_cache =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyMeasuredItemCache::default())))
            .with(Clone::clone);
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
        state_identity: lazy_list_state_identity(&state),
        is_vertical: true,
        reverse_scrolling: spec.reverse_layout,
    });
    let overscroll = motion_context.overscroll();

    let measured_item_cache_for_policy = Rc::clone(&measured_item_cache);
    let overscroll_for_policy = overscroll.clone();
    let policy: Rc<MeasurePolicy> = cranpose_core::remember(move || {
        let config_ref = Rc::clone(&config_cell);
        let content_ref = Rc::clone(&content_cell);
        let measured_item_cache = Rc::clone(&measured_item_cache_for_policy);
        let overscroll = overscroll_for_policy.clone();
        let policy: Rc<MeasurePolicy> = Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = config_ref.borrow();
                measure_lazy_list_internal(
                    scope,
                    constraints,
                    true,
                    content.content(),
                    LazyListMeasureContext {
                        state: &state,
                        config: &config,
                        measured_item_cache: &measured_item_cache,
                        overscroll: &overscroll,
                    },
                )
            },
        );
        policy
    })
    .with(|p| Rc::clone(p));
    let list_state_id = lazy_list_state_identity(&state);

    let scroll_modifier = grid::grid_semantics(
        modifier.clip_to_bounds().lazy_vertical_scroll_with_context(
            state,
            spec.reverse_layout,
            padding.main,
            motion_context,
        ),
        semantics_grid.as_ref(),
        true,
    );

    let node_id = cranpose_core::with_current_composer(|composer| {
        composer.with_key(&(list_state_id, "LazyColumnNode"), |composer| {
            composer.emit_node({
                let scroll_modifier = scroll_modifier.clone();
                let policy = Rc::clone(&policy);
                move || SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy)
            })
        })
    });
    let captured_context =
        cranpose_core::with_current_composer(cranpose_core::Composer::capture_composition_context);
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        let inputs_changed =
            refresh_content || config_changed || !node.modifier().structural_eq(&scroll_modifier);
        if inputs_changed || caller_modifier_changed {
            node.set_modifier(scroll_modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        node.set_layout_direction(direction);
        if inputs_changed {
            measured_item_cache.borrow_mut().clear();
            node.invalidate_subcomposition();
        }
    }) {
        debug_assert!(false, "failed to update LazyColumn node: {err}");
    }
    bind_layout_invalidation_callback(state, overscroll, list_state_id, node_id);

    node_id
}

/// Internal implementation for LazyRow that takes pre-built content.
///
/// Users should prefer the DSL-based [`LazyRow`] function instead.
fn LazyRowImpl(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyRowSpec,
    grid: Option<grid::CellLayout>,
    content: LazyListContentHandle,
) -> NodeId {
    use std::cell::RefCell;

    let content_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyListContentHandle::empty())))
            .with(Clone::clone);

    let refresh_content = !lazy_list_state_only_recomposition(&state);
    if refresh_content {
        *content_cell.borrow_mut() = content;
    }
    let caller_modifier_changed = super::layout::caller_modifier_changed(&modifier);

    let composed_density = crate::density::density();
    let direction = crate::layout_direction();
    let is_rtl = direction.is_rtl();
    let reverse_scrolling = spec.reverse_layout ^ is_rtl;
    let padding = resolve_content_padding(
        spec.content_padding,
        false,
        spec.reverse_layout,
        direction,
        composed_density.density(),
    );
    let semantics_grid = grid.clone();
    let config = LazyListLayoutConfig {
        measure: LazyListMeasureConfig {
            is_vertical: false,
            is_rtl,
            reverse_layout: spec.reverse_layout,
            before_content_padding: padding.before,
            after_content_padding: padding.after,
            spacing: spec
                .horizontal_arrangement
                .spacing(composed_density.density()),
            beyond_bounds_item_count: spec.beyond_bounds_item_count,
            vertical_arrangement: None,
            horizontal_arrangement: Some(spec.horizontal_arrangement),
        },
        cross_padding: padding.cross,
        grid,
    };
    let config_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(config.clone()))).with(Clone::clone);
    let config_changed = {
        let mut current = config_cell.borrow_mut();
        let changed = *current != config;
        if changed {
            *current = config;
        }
        changed
    };
    let measured_item_cache =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyMeasuredItemCache::default())))
            .with(Clone::clone);
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
        state_identity: lazy_list_state_identity(&state),
        is_vertical: false,
        reverse_scrolling,
    });
    let overscroll = motion_context.overscroll();

    let measured_item_cache_for_policy = Rc::clone(&measured_item_cache);
    let overscroll_for_policy = overscroll.clone();
    let policy: Rc<MeasurePolicy> = cranpose_core::remember(move || {
        let config_ref = Rc::clone(&config_cell);
        let content_ref = Rc::clone(&content_cell);
        let measured_item_cache = Rc::clone(&measured_item_cache_for_policy);
        let overscroll = overscroll_for_policy.clone();
        let policy: Rc<MeasurePolicy> = Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = config_ref.borrow();
                measure_lazy_list_internal(
                    scope,
                    constraints,
                    false,
                    content.content(),
                    LazyListMeasureContext {
                        state: &state,
                        config: &config,
                        measured_item_cache: &measured_item_cache,
                        overscroll: &overscroll,
                    },
                )
            },
        );
        policy
    })
    .with(|p| Rc::clone(p));
    let list_state_id = lazy_list_state_identity(&state);

    let scroll_modifier = grid::grid_semantics(
        modifier
            .clip_to_bounds()
            .lazy_horizontal_scroll_with_context(
                state,
                reverse_scrolling,
                padding.main,
                motion_context,
            ),
        semantics_grid.as_ref(),
        false,
    );

    let node_id = cranpose_core::with_current_composer(|composer| {
        composer.with_key(&(list_state_id, "LazyRowNode"), |composer| {
            composer.emit_node({
                let scroll_modifier = scroll_modifier.clone();
                let policy = Rc::clone(&policy);
                move || SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy)
            })
        })
    });
    let captured_context =
        cranpose_core::with_current_composer(cranpose_core::Composer::capture_composition_context);
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        let inputs_changed =
            refresh_content || config_changed || !node.modifier().structural_eq(&scroll_modifier);
        if inputs_changed || caller_modifier_changed {
            node.set_modifier(scroll_modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        node.set_layout_direction(direction);
        if inputs_changed {
            measured_item_cache.borrow_mut().clear();
            node.invalidate_subcomposition();
        }
    }) {
        debug_assert!(false, "failed to update LazyRow node: {err}");
    }
    bind_layout_invalidation_callback(state, overscroll, list_state_id, node_id);

    node_id
}

#[composable]
fn LazyColumnNode(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyColumnSpec,
    grid: Option<grid::CellLayout>,
    content: LazyListContentHandle,
) -> NodeId {
    cranpose_core::debug_label_current_scope("LazyColumnNode");

    let viewport = cranpose_core::remember(|| {
        Rc::new(Cell::new(cranpose_ui_graphics::WindowCoordinates::default()))
    })
    .with(Rc::clone);
    let responder = {
        let viewport = Rc::clone(&viewport);
        cranpose_core::remember(move || {
            let viewport = Rc::clone(&viewport);
            crate::bring_into_view::BringIntoViewResponder::new(move |caret, ime_bottom| {
                let vp = viewport.get();
                if vp.size.width <= 0.0 || vp.size.height <= 0.0 {
                    return;
                }
                let delta =
                    crate::bring_into_view::local_scroll_delta_to_reveal(caret, vp, ime_bottom);
                if delta.abs() > 0.5 {
                    state.dispatch_scroll_delta(-delta);
                }
            })
        })
        .with(|r| r.clone())
    };
    let modifier = modifier.report_window_coordinates(Rc::clone(&viewport));

    let mut node: Option<NodeId> = None;
    {
        let node_slot = &mut node;
        cranpose_core::CompositionLocalProvider(
            [crate::bring_into_view::local_bring_into_view_responder().provides(Some(responder))],
            move || {
                *node_slot = Some(LazyColumnImpl(modifier, state, spec, grid, content));
            },
        );
    }
    node.expect("LazyColumnImpl emits a node")
}

#[composable]
fn LazyRowNode(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyRowSpec,
    grid: Option<grid::CellLayout>,
    content: LazyListContentHandle,
) -> NodeId {
    cranpose_core::debug_label_current_scope("LazyRowNode");
    LazyRowImpl(modifier, state, spec, grid, content)
}

/// Composes and measures the rows needed for a vertical viewport. The content
/// closure declares rows through [`LazyListScope`](cranpose_foundation::lazy::LazyListScope).
/// Use stable keys for item identity across changes and content types for distinct row shapes.
/// The state retains the scroll position; the spec sets item space and content insets.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Messages() {
///     let state = rememberLazyListState();
///     LazyColumn(
///         Modifier::empty().fill_max_size(),
///         state,
///         LazyColumnSpec::default(),
///         |scope| {
///             scope.items(LazyItems::new(1000), |index| {
///                 Text(
///                     format!("Message {index}"),
///                     Modifier::empty().padding(16.0),
///                     TextStyle::default(),
///                 );
///             });
///         },
///     );
/// }
/// ```
pub fn LazyColumn<F>(
    modifier: Modifier,
    state: LazyListState,
    spec: LazyColumnSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    LazyColumnNode(
        modifier,
        state,
        spec,
        None,
        LazyListContentHandle::build(content),
    )
}

/// Composes and measures the items needed for a horizontal viewport.
/// Use [`LazyColumn`] for vertical lists.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Choices() {
///     let state = rememberLazyListState();
///     LazyRow(Modifier::empty(), state, LazyRowSpec::default(), |scope| {
///         scope.items(LazyItems::new(10), |index| {
///             Text(
///                 format!("Choice {index}"),
///                 Modifier::empty().padding(8.0),
///                 TextStyle::default(),
///             );
///         });
///     });
/// }
/// ```
pub fn LazyRow<F>(modifier: Modifier, state: LazyListState, spec: LazyRowSpec, content: F) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    LazyRowNode(
        modifier,
        state,
        spec,
        None,
        LazyListContentHandle::build(content),
    )
}

#[cfg(test)]
#[path = "tests/lazy_list_tests.rs"]
mod tests;
