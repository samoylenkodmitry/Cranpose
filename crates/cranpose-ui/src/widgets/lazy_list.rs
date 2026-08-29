//! LazyColumn and LazyRow widget implementations.
//!
//! Provides virtualized scrolling lists that only compose visible items,
//! matching Jetpack Compose's `LazyColumn` and `LazyRow` APIs.

#![allow(non_snake_case)]
#![allow(dead_code)] // Some public widget entry points are exercised by downstream apps.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
};

use cranpose_core::{NodeId, SlotId};
use cranpose_foundation::lazy::{
    LazyListIntervalContent, LazyListMeasureConfig, LazyListMeasureResult, LazyListMeasuredItem,
    LazyListState, SmallNodeVec, SmallOffsetVec, measure_lazy_list,
    measure_lazy_list_with_beyond_bounds_policy,
};
// Re-export from foundation - single source of truth
pub use cranpose_foundation::lazy::{LazyListItemInfo, LazyListLayoutInfo};
use cranpose_ui_layout::{Constraints, LinearArrangement, MeasureResult};
use smallvec::SmallVec;
use web_time::Instant;

use crate::{
    composable,
    layout::MeasuredNode,
    modifier::{Modifier, Size},
    scroll::{OverscrollEffect, ScrollMotionContextKey, scroll_motion_context_for_key},
    subcompose_layout::{
        MeasurePolicy, Placement, SubcomposeChild, SubcomposeLayoutNode, SubcomposeMeasureScope,
        SubcomposeMeasureScopeImpl,
    },
};

const EXPENSIVE_RETAINED_REUSABLE_SLOTS: usize = 128;
const ACTIVE_SCROLL_UNCACHED_BEYOND_BOUNDS_FRONTIER: usize = 4;

#[derive(Clone, Copy)]
struct LazyItemMeasureContext {
    index: usize,
    key_slot_id: u64,
    content_type: Option<u64>,
    is_vertical: bool,
    cross_axis_size: f32,
    measure_start: Instant,
}

/// Specification for LazyColumn layout behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct LazyColumnSpec {
    /// Vertical arrangement for spacing between items.
    pub vertical_arrangement: LinearArrangement,
    /// Content padding before the first item.
    pub content_padding_top: f32,
    /// Content padding after the last item.
    pub content_padding_bottom: f32,
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
            content_padding_top: 0.0,
            content_padding_bottom: 0.0,
            beyond_bounds_item_count: 2,
            reverse_layout: false,
        }
    }
}

impl LazyColumnSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    pub fn content_padding(mut self, top: f32, bottom: f32) -> Self {
        self.content_padding_top = top;
        self.content_padding_bottom = bottom;
        self
    }

    /// Sets uniform content padding for top and bottom.
    pub fn content_padding_all(mut self, padding: f32) -> Self {
        self.content_padding_top = padding;
        self.content_padding_bottom = padding;
        self
    }

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
    /// Content padding before the first item.
    pub content_padding_start: f32,
    /// Content padding after the last item.
    pub content_padding_end: f32,
    /// Number of items to compose beyond the visible bounds.
    pub beyond_bounds_item_count: usize,
    /// Whether to reverse the layout direction (end-to-start).
    pub reverse_layout: bool,
}

impl Default for LazyRowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            content_padding_start: 0.0,
            content_padding_end: 0.0,
            beyond_bounds_item_count: 2,
            reverse_layout: false,
        }
    }
}

impl LazyRowSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    pub fn content_padding(mut self, start: f32, end: f32) -> Self {
        self.content_padding_start = start;
        self.content_padding_end = end;
        self
    }

    /// Sets uniform content padding for start and end.
    pub fn content_padding_all(mut self, padding: f32) -> Self {
        self.content_padding_start = padding;
        self.content_padding_end = padding;
        self
    }

    pub fn reverse_layout(mut self, reverse: bool) -> Self {
        self.reverse_layout = reverse;
        self
    }
}

struct LazyListItemMeasureInputs<'a> {
    is_vertical: bool,
    cross_axis_size: f32,
    content: &'a LazyListIntervalContent,
    state: &'a LazyListState,
    measured_item_cache: &'a Rc<RefCell<LazyMeasuredItemCache>>,
}

fn measure_lazy_list_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    index: usize,
    inputs: &LazyListItemMeasureInputs<'_>,
    retained_measurement_batch: &mut Vec<Rc<MeasuredNode>>,
) -> LazyListMeasuredItem {
    let measure_start = Instant::now();
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
        measure_start,
    };

    scope.update_content_type(slot_id, content_type);

    let cached_candidate = {
        inputs
            .measured_item_cache
            .borrow_mut()
            .candidate(index, key_slot_id, content_type)
    };
    if let Some(cached) = cached_candidate {
        inputs
            .measured_item_cache
            .borrow_mut()
            .record_candidate_hit();
        if let Some((root_children, children_match)) =
            scope.activate_exact_retained_slot_with_known_children(slot_id, &cached.item.node_ids)
        {
            let children_are_clean = !scope.children_need_relayout(&root_children);
            if children_match && children_are_clean {
                retained_measurement_batch.extend(cached.retained_children.iter().cloned());
                inputs.measured_item_cache.borrow_mut().record_exact_reuse();
                return cached.item;
            }
            inputs.measured_item_cache.borrow_mut().remove(index);
            if children_match {
                inputs
                    .measured_item_cache
                    .borrow_mut()
                    .record_dirty_children();
            } else {
                inputs.measured_item_cache.borrow_mut().record_exact_miss();
            }
            return measure_lazy_list_children(
                scope,
                root_children,
                inputs.measured_item_cache,
                item_context,
            );
        } else {
            inputs.measured_item_cache.borrow_mut().record_exact_miss();
            inputs.measured_item_cache.borrow_mut().remove(index);
        }
    } else {
        inputs
            .measured_item_cache
            .borrow_mut()
            .record_candidate_miss();
    }

    // Only a user key is an identity: an index key names the position, which is
    // exactly the identity that leaks per-item state onto the wrong row when the
    // list shifts. See `crate::lazy_item`.
    let item_identity = key.is_user_key().then_some(key_slot_id);
    let Some(item_content) = inputs
        .content
        .with_interval(index, |local_index, interval| {
            let content = Rc::clone(&interval.content);
            move || {
                crate::lazy_item::ProvideLazyItemKey(item_identity, || (content)(local_index));
            }
        })
    else {
        return LazyListMeasuredItem::new(index, key_slot_id, content_type, 1.0, 0.0);
    };
    let root_children = scope.subcompose(slot_id, item_content);

    let was_reused = scope.was_last_slot_reused().unwrap_or(false);
    inputs.state.record_composition(was_reused);

    let root_node_ids: SmallNodeVec = root_children
        .iter()
        .map(|child| child.node_id() as u64)
        .collect();

    if let Some(cached) = inputs.measured_item_cache.borrow_mut().get(
        index,
        key_slot_id,
        content_type,
        &root_node_ids,
    ) {
        if !scope.children_need_relayout(&root_children) {
            scope.register_retained_measurements(&cached.retained_children);
            return cached.item;
        }
        inputs.measured_item_cache.borrow_mut().remove(index);
    }

    measure_lazy_list_children(
        scope,
        root_children,
        inputs.measured_item_cache,
        item_context,
    )
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
fn register_visible_lazy_list_child_measurements(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    visible_items: &[LazyListMeasuredItem],
    is_vertical: bool,
    cross_axis_size: f32,
) {
    let child_constraints = lazy_list_child_constraints(is_vertical, cross_axis_size);
    scope.ensure_cached_measurement_node_ids(
        visible_items
            .iter()
            .flat_map(|item| item.node_ids.iter())
            .filter_map(|&node_id| NodeId::try_from(node_id).ok()),
        child_constraints,
    );
}

fn measure_lazy_list_children(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    root_children: Vec<SubcomposeChild>,
    measured_item_cache: &Rc<RefCell<LazyMeasuredItemCache>>,
    context: LazyItemMeasureContext,
) -> LazyListMeasuredItem {
    let child_constraints =
        lazy_list_child_constraints(context.is_vertical, context.cross_axis_size);

    let mut total_main_size: f32 = 0.0;
    let mut max_cross_size: f32 = 0.0;
    let mut node_ids: SmallNodeVec = SmallVec::with_capacity(root_children.len());
    let mut child_offsets: SmallOffsetVec = SmallVec::new();
    let mut retained_children: SmallVec<[Rc<MeasuredNode>; 4]> =
        SmallVec::with_capacity(root_children.len());

    for child in root_children {
        let (placeable, retained) = scope.measure_retained(child, child_constraints);
        let size = retained
            .as_ref()
            .map(|measured| measured.size())
            .unwrap_or_else(|| Size {
                width: placeable.width(),
                height: placeable.height(),
            });
        let (main, cross) = if context.is_vertical {
            (size.height, size.width)
        } else {
            (size.width, size.height)
        };

        child_offsets.push(total_main_size);
        node_ids.push(child.node_id() as u64);
        if let Some(retained) = retained {
            retained_children.push(retained);
        }

        total_main_size += main;
        max_cross_size = max_cross_size.max(cross);
    }

    let main_axis_size = total_main_size.max(1.0);
    let mut item = LazyListMeasuredItem::new(
        context.index,
        context.key_slot_id,
        context.content_type,
        main_axis_size,
        max_cross_size,
    );
    item.node_ids = node_ids;
    item.child_offsets = child_offsets;

    measured_item_cache
        .borrow_mut()
        .insert(item.clone(), retained_children);
    let elapsed = context.measure_start.elapsed();
    if cranpose_core::env_flag!("CRANPOSE_LAZY_ITEM_TELEMETRY") {
        log::warn!(
            "[lazy-item-telemetry] index={} children={} main={:.2} cross={:.2} elapsed_ms={:.2}",
            context.index,
            item.node_ids.len(),
            item.main_axis_size,
            item.cross_axis_size,
            elapsed.as_secs_f64() * 1000.0
        );
    }
    measured_item_cache.borrow_mut().record_uncached_measure();
    item
}

fn recycle_forward_skipped_active_slots(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    first_measured_index: usize,
    scroll_delta: f32,
) -> bool {
    if scroll_delta >= -0.001 || first_measured_index == 0 {
        return false;
    }

    scope.recycle_active_slots_where(|slot_id| {
        content
            .get_index_by_slot_id(slot_id.raw())
            .is_some_and(|index| index < first_measured_index)
    });
    true
}

/// Internal helper to create a lazy list measure policy.
struct LazyListMeasureContext<'a> {
    state: &'a LazyListState,
    config: &'a LazyListMeasureConfig,
    measured_item_cache: &'a Rc<RefCell<LazyMeasuredItemCache>>,
    overscroll: &'a OverscrollEffect,
}

fn measure_lazy_list_internal(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    is_vertical: bool,
    content: &LazyListIntervalContent,
    context: LazyListMeasureContext<'_>,
) -> MeasureResult {
    let state = context.state;
    let config = context.config;
    let measured_item_cache = context.measured_item_cache;
    let overscroll = context.overscroll;
    let raw_viewport_size = if is_vertical {
        constraints.max_height
    } else {
        constraints.max_width
    };
    let cross_axis_size = if is_vertical {
        constraints.max_width
    } else {
        constraints.max_height
    };

    let items_count = content.item_count();
    let retained_reusable_slots = EXPENSIVE_RETAINED_REUSABLE_SLOTS;
    scope.set_reusable_pool_limits(retained_reusable_slots, retained_reusable_slots);
    measured_item_cache
        .borrow_mut()
        .retain_constraint_scope(is_vertical, cross_axis_size);
    // Scroll position stability: if items were added/removed before the first visible,
    // find the item by key and adjust scroll position (JC's updateScrollPositionIfTheFirstItemWasMoved)
    if items_count > 0 {
        // Scroll position stability: try O(1) range search first, fall back to O(N) global search
        // This matches the performance-optimal pattern: most items are found within the range
        let range = state.nearest_range();
        state.update_scroll_position_if_item_moved(items_count, |slot_id| {
            content
                .get_index_by_slot_id_in_range(slot_id, range.clone())
                .or_else(|| content.get_index_by_slot_id(slot_id))
        });
        // Note: nearest range is automatically updated by scroll_position when index changes
    }

    // Capture scroll delta for direction inference BEFORE measurement consumes it.
    // This is more accurate than comparing first visible index, especially for:
    // - Scrolling within the same item (partial scroll)
    // - Variable height items where scroll offset changes without index change
    let scroll_delta_for_direction = state.peek_scroll_delta();
    let skipped_slots_recycled = Cell::new(false);
    let mut retained_measurement_batch = Vec::new();
    let item_measure_inputs = LazyListItemMeasureInputs {
        is_vertical,
        cross_axis_size,
        content,
        state,
        measured_item_cache,
    };

    // Run the lazy list measurement algorithm
    let measure_item = |index: usize| -> LazyListMeasuredItem {
        if !skipped_slots_recycled.get()
            && recycle_forward_skipped_active_slots(
                scope,
                content,
                index,
                scroll_delta_for_direction,
            )
        {
            skipped_slots_recycled.set(true);
        }
        measure_lazy_list_item(
            scope,
            index,
            &item_measure_inputs,
            &mut retained_measurement_batch,
        )
    };
    let mut measure_item = measure_item;
    let active_scroll = scroll_delta_for_direction.abs() > 0.001;
    let result = if active_scroll {
        let measured_item_cache_for_policy = Rc::clone(measured_item_cache);
        let uncached_beyond_frontier = Cell::new(ACTIVE_SCROLL_UNCACHED_BEYOND_BOUNDS_FRONTIER);
        measure_lazy_list_with_beyond_bounds_policy(
            items_count,
            state,
            raw_viewport_size,
            cross_axis_size,
            config,
            &mut measure_item,
            |index| {
                let key_slot_id = content.get_key(index).to_slot_id();
                let content_type = content.get_content_type(index);
                if measured_item_cache_for_policy.borrow().has_candidate(
                    index,
                    key_slot_id,
                    content_type,
                ) {
                    return true;
                }
                let remaining = uncached_beyond_frontier.get();
                if remaining == 0 {
                    return false;
                }
                uncached_beyond_frontier.set(remaining - 1);
                true
            },
        )
    } else {
        measure_lazy_list(
            items_count,
            state,
            raw_viewport_size,
            cross_axis_size,
            config,
            &mut measure_item,
        )
    };
    if !retained_measurement_batch.is_empty() {
        scope.register_retained_measurements(&retained_measurement_batch);
    }
    register_visible_lazy_list_child_measurements(
        scope,
        &result.visible_items,
        is_vertical,
        cross_axis_size,
    );
    log_lazy_cache_telemetry(&result, measured_item_cache);
    let effective_viewport_size = result.viewport_size;
    overscroll.set_dimension(effective_viewport_size);

    // Cache measured item sizes for better scroll estimation
    state.cache_item_sizes(
        result
            .visible_items
            .iter()
            .map(|item| (item.index, item.main_axis_size)),
    );
    // Update stats: count only items WITHIN viewport, not beyond-bounds buffer
    let truly_visible_count = result
        .visible_items
        .iter()
        .filter(|item| {
            // Item is visible if any part of it is within viewport bounds
            let item_end = item.offset + item.main_axis_size;
            item.offset < effective_viewport_size && item_end > 0.0
        })
        .count();
    // Get reusable slot count from SubcomposeState (the single source of truth)
    let in_pool = scope.reusable_slots_count();
    state.update_stats(truly_visible_count, in_pool);

    if !result.visible_items.is_empty() {
        state.record_scroll_direction(scroll_delta_for_direction);
    }

    let resolve_main_axis = |content_size: f32, min: f32, max: f32| {
        if max.is_finite() {
            content_size.clamp(min, max)
        } else {
            content_size.min(effective_viewport_size).max(min)
        }
    };

    // Report size that respects BOTH min and max constraints.
    // - If content < min: expand to min (e.g., fillMaxSize)
    // - If content > max: clamp to max (enables scrolling)
    // - Otherwise: use content size (shrink-wrap)
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

    scope.layout_with_placement_builder(width, height, |placements| {
        push_lazy_list_placements(
            placements,
            &result.visible_items,
            items_count,
            is_vertical,
            effective_viewport_size,
            config,
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

fn get_spacing(arrangement: LinearArrangement) -> f32 {
    match arrangement {
        LinearArrangement::SpacedBy(spacing) => spacing,
        _ => 0.0,
    }
}

fn bind_layout_invalidation_callback(
    state: LazyListState,
    overscroll: OverscrollEffect,
    list_state_id: usize,
    node_id: NodeId,
) {
    let callback_owner =
        cranpose_core::remember(|| Rc::new(RefCell::new(None::<u64>))).with(|cell| cell.clone());
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
        cranpose_core::remember(|| Rc::new(RefCell::new(None::<u64>))).with(|cell| cell.clone());
    let overscroll_callback_id = overscroll.add_invalidate_callback(Box::new(move || {
        let _ = crate::render_state::enter_app_context_by_id(app_context_id, || {
            crate::schedule_measure_repass(node_id);
            crate::schedule_semantics_invalidation(node_id);
        });
    }));
    if let Some(previous_id) = overscroll_callback_owner.replace(Some(overscroll_callback_id)) {
        overscroll.remove_invalidate_callback(previous_id);
    }

    cranpose_core::DisposableEffect!(
        (list_state_id, node_id, callback_id, overscroll_callback_id),
        move |scope| {
            scope.on_dispose(move || {
                if let Some(callback_id) = callback_id {
                    state.remove_invalidate_callback(callback_id);
                }
                overscroll.remove_invalidate_callback(overscroll_callback_id);
            })
        }
    );
}

#[derive(Clone)]
struct LazyListContentHandle(Rc<LazyListIntervalContent>);

impl LazyListContentHandle {
    fn new(content: LazyListIntervalContent) -> Self {
        Self(Rc::new(content))
    }

    fn empty() -> Self {
        Self::new(LazyListIntervalContent::new())
    }

    fn content(&self) -> &LazyListIntervalContent {
        self.0.as_ref()
    }
}

impl PartialEq for LazyListContentHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

const MEASURED_ITEM_CACHE_CAPACITY: usize = 4096;

#[derive(Default)]
struct LazyMeasuredItemCache {
    is_vertical: bool,
    cross_axis_bits: u32,
    telemetry: LazyCacheTelemetry,
    entries: HashMap<usize, CachedLazyMeasuredItem>,
    order: VecDeque<usize>,
}

#[derive(Clone)]
struct CachedLazyMeasuredItem {
    item: LazyListMeasuredItem,
    retained_children: SmallVec<[Rc<MeasuredNode>; 4]>,
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
        self.order.clear();
    }

    fn get(
        &mut self,
        index: usize,
        key: u64,
        content_type: Option<u64>,
        node_ids: &SmallNodeVec,
    ) -> Option<CachedLazyMeasuredItem> {
        let cached = self.entries.get(&index)?;
        if cached.item.key != key
            || cached.item.content_type != content_type
            || cached.item.node_ids != *node_ids
            || cached.retained_children.len() != cached.item.node_ids.len()
        {
            self.entries.remove(&index);
            return None;
        }
        Some(cached.clone())
    }

    fn candidate(
        &mut self,
        index: usize,
        key: u64,
        content_type: Option<u64>,
    ) -> Option<CachedLazyMeasuredItem> {
        let cached = self.entries.get(&index)?;
        if cached.item.key != key
            || cached.item.content_type != content_type
            || cached.retained_children.len() != cached.item.node_ids.len()
        {
            self.entries.remove(&index);
            return None;
        }
        Some(cached.clone())
    }

    fn has_candidate(&self, index: usize, key: u64, content_type: Option<u64>) -> bool {
        self.entries.get(&index).is_some_and(|cached| {
            cached.item.key == key
                && cached.item.content_type == content_type
                && cached.retained_children.len() == cached.item.node_ids.len()
        })
    }

    fn remove(&mut self, index: usize) {
        self.entries.remove(&index);
    }

    fn insert(
        &mut self,
        item: LazyListMeasuredItem,
        retained_children: SmallVec<[Rc<MeasuredNode>; 4]>,
    ) {
        let index = item.index;
        let cached = CachedLazyMeasuredItem {
            item,
            retained_children,
        };
        if self.entries.insert(index, cached).is_none() {
            self.order.push_back(index);
        }
        while self.entries.len() > MEASURED_ITEM_CACHE_CAPACITY {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
        }
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
    #[cfg(test)]
    eprintln!("{message}");
}

fn normalized_axis_bits(size: f32) -> u32 {
    if size.is_finite() && size >= 0.0 {
        size.to_bits()
    } else {
        f32::INFINITY.to_bits()
    }
}

/// Writes placements for measured lazy list items.
///
/// This helper encapsulates the logic for:
/// - Applying arrangement when all items fit (hasSpareSpace in JC)
/// - Using sequential positioning during scrolling
fn push_lazy_list_placements(
    placements: &mut Vec<Placement>,
    visible_items: &[LazyListMeasuredItem],
    items_count: usize,
    is_vertical: bool,
    viewport_size: f32,
    config: &LazyListMeasureConfig,
) {
    use cranpose_ui_layout::Arrangement;

    placements.clear();
    placements.reserve(visible_items.iter().map(|item| item.node_ids.len()).sum());

    let arrangement = if is_vertical {
        config
            .vertical_arrangement
            .unwrap_or(LinearArrangement::Start)
    } else {
        config
            .horizontal_arrangement
            .unwrap_or(LinearArrangement::Start)
    };

    // Check if we should apply arrangement:
    // 1. All items are visible (visible_items.len() == total items)
    // 2. Content is smaller than viewport (hasSpareSpace)
    // 3. Arrangement is not sequential (Start or SpacedBy)
    let spacing = get_spacing(arrangement);
    let total_item_size: f32 = visible_items.iter().map(|i| i.main_axis_size).sum::<f32>()
        + (items_count.saturating_sub(1) as f32) * spacing;
    // Account for content padding when checking spare space (JC pattern)
    // Clamp to 0.0 to handle edge case where padding exceeds viewport
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
        // Apply arrangement to compute final positions
        // JC: density.arrange(mainAxisLayoutSize, sizes, offsets)
        let content_offset = config.before_content_padding;

        let sizes: SmallVec<[f32; 32]> = visible_items.iter().map(|i| i.main_axis_size).collect();
        let mut positions: SmallVec<[f32; 32]> = SmallVec::from_elem(0.0, sizes.len());
        arrangement.arrange(available_main_axis, &sizes, &mut positions);

        for (item, &pos) in visible_items.iter().zip(positions.iter()) {
            for (&nid, &child_offset) in item.node_ids.iter().zip(item.child_offsets.iter()) {
                let node_id: NodeId = nid as NodeId;
                let item_size = item.main_axis_size;

                let placement = if is_vertical {
                    let y = if config.reverse_layout {
                        viewport_size - (content_offset + pos) - item_size + child_offset
                    } else {
                        content_offset + pos + child_offset
                    };
                    Placement::new(node_id, 0.0, y, 0)
                } else {
                    let x = if config.reverse_layout {
                        viewport_size - (content_offset + pos) - item_size + child_offset
                    } else {
                        content_offset + pos + child_offset
                    };
                    Placement::new(node_id, x, 0.0, 0)
                };
                placements.push(placement);
            }
        }
    } else {
        // Use sequential offsets from measurement (scrolling case)
        for item in visible_items {
            for (&nid, &child_offset) in item.node_ids.iter().zip(item.child_offsets.iter()) {
                let node_id: NodeId = nid as NodeId;
                let item_size = item.main_axis_size;

                let placement = if is_vertical {
                    let y = if config.reverse_layout {
                        viewport_size - item.offset - item_size + child_offset
                    } else {
                        item.offset + child_offset
                    };
                    Placement::new(node_id, 0.0, y, 0)
                } else {
                    let x = if config.reverse_layout {
                        viewport_size - item.offset - item_size + child_offset
                    } else {
                        item.offset + child_offset
                    };
                    Placement::new(node_id, x, 0.0, 0)
                };
                placements.push(placement);
            }
        }
    }
}

fn lazy_list_state_identity(state: &LazyListState) -> usize {
    // The remembered state stores its inner payload behind an `Rc`, so this allocation address
    // remains stable for the lifetime of the live state handle and is safe to use as a list key.
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
    content: LazyListContentHandle,
) -> NodeId {
    use std::cell::RefCell;

    // Use remember to keep a shared RefCell for content that persists across recompositions
    // This allows updating the content on each recomposition while reusing the same node/policy
    let content_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyListContentHandle::empty())))
            .with(|cell| cell.clone());

    let refresh_content = !lazy_list_state_only_recomposition(&state);
    if refresh_content {
        *content_cell.borrow_mut() = content;
    }

    let config = LazyListMeasureConfig {
        is_vertical: true,
        reverse_layout: spec.reverse_layout,
        before_content_padding: spec.content_padding_top,
        after_content_padding: spec.content_padding_bottom,
        spacing: get_spacing(spec.vertical_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: Some(spec.vertical_arrangement),
        horizontal_arrangement: None,
    };
    let config_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(config.clone()))).with(|cell| cell.clone());
    let config_changed = {
        let mut current = config_cell.borrow_mut();
        let changed = *current != config;
        if changed {
            *current = config.clone();
        }
        changed
    };
    let measured_item_cache =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyMeasuredItemCache::default())))
            .with(|cache| cache.clone());
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
        state_identity: lazy_list_state_identity(&state),
        is_vertical: true,
        reverse_scrolling: spec.reverse_layout,
    });
    let overscroll = motion_context.overscroll();

    // Create measure policy with stable identity using remember.
    // The policy reads latest values via state references, so it can be memoized.
    let content_for_policy = content_cell.clone();
    let measured_item_cache_for_policy = measured_item_cache.clone();
    let overscroll_for_policy = overscroll.clone();
    let policy: Rc<MeasurePolicy> = cranpose_core::remember(move || {
        let config_ref = config_cell.clone();
        let content_ref = content_for_policy.clone();
        let measured_item_cache = measured_item_cache_for_policy.clone();
        let overscroll = overscroll_for_policy.clone();
        let policy: Rc<MeasurePolicy> = Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = config_ref.borrow().clone();
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
    .with(|p| p.clone());
    let list_state_id = lazy_list_state_identity(&state);

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier.clip_to_bounds().lazy_vertical_scroll_with_context(
        state,
        spec.reverse_layout,
        motion_context,
    );

    // Create and register the subcompose layout node with the composer
    let node_id = cranpose_core::with_current_composer(|composer| {
        composer.with_key(&(list_state_id, "LazyColumnNode"), |composer| {
            composer.emit_node({
                let scroll_modifier = scroll_modifier.clone();
                let policy = Rc::clone(&policy);
                move || SubcomposeLayoutNode::with_content_type_policy(scroll_modifier, policy)
            })
        })
    });
    // Items composed during measure inherit the call-site locals and source
    // scope, keeping overlays wired and secondary callbacks lifetime-bound.
    let captured_context =
        cranpose_core::with_current_composer(|composer| composer.capture_composition_context());
    // Read while the composition is still running, same as `Layout`: measurement
    // happens after it and cannot reach a composition local. Reading here also
    // subscribes, so a subtree given a different grid recomposes and re-captures.
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        let modifier_changed = !node.modifier().structural_eq(&scroll_modifier);
        if refresh_content || config_changed || modifier_changed {
            node.set_modifier(scroll_modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        if refresh_content || config_changed || modifier_changed {
            measured_item_cache.borrow_mut().clear();
            node.request_measure_recompose();
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
    content: LazyListContentHandle,
) -> NodeId {
    use std::cell::RefCell;

    // Use remember to keep a shared RefCell for content that persists across recompositions
    let content_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyListContentHandle::empty())))
            .with(|cell| cell.clone());

    let refresh_content = !lazy_list_state_only_recomposition(&state);
    if refresh_content {
        *content_cell.borrow_mut() = content;
    }

    let config = LazyListMeasureConfig {
        is_vertical: false,
        reverse_layout: spec.reverse_layout,
        before_content_padding: spec.content_padding_start,
        after_content_padding: spec.content_padding_end,
        spacing: get_spacing(spec.horizontal_arrangement),
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        vertical_arrangement: None,
        horizontal_arrangement: Some(spec.horizontal_arrangement),
    };
    let config_cell =
        cranpose_core::remember(|| Rc::new(RefCell::new(config.clone()))).with(|cell| cell.clone());
    let config_changed = {
        let mut current = config_cell.borrow_mut();
        let changed = *current != config;
        if changed {
            *current = config.clone();
        }
        changed
    };
    let measured_item_cache =
        cranpose_core::remember(|| Rc::new(RefCell::new(LazyMeasuredItemCache::default())))
            .with(|cache| cache.clone());
    let motion_context = scroll_motion_context_for_key(ScrollMotionContextKey::LazyList {
        state_identity: lazy_list_state_identity(&state),
        is_vertical: false,
        reverse_scrolling: spec.reverse_layout,
    });
    let overscroll = motion_context.overscroll();

    // Create measure policy with stable identity using remember.
    let content_for_policy = content_cell.clone();
    let measured_item_cache_for_policy = measured_item_cache.clone();
    let overscroll_for_policy = overscroll.clone();
    let policy: Rc<MeasurePolicy> = cranpose_core::remember(move || {
        let config_ref = config_cell.clone();
        let content_ref = content_for_policy.clone();
        let measured_item_cache = measured_item_cache_for_policy.clone();
        let overscroll = overscroll_for_policy.clone();
        let policy: Rc<MeasurePolicy> = Rc::new(
            move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                let content = content_ref.borrow();
                let config = config_ref.borrow().clone();
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
    .with(|p| p.clone());
    let list_state_id = lazy_list_state_identity(&state);

    // Apply clipping and scroll gesture handling to modifier
    let scroll_modifier = modifier
        .clip_to_bounds()
        .lazy_horizontal_scroll_with_context(state, spec.reverse_layout, motion_context);

    // Create and register the subcompose layout node with the composer
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
        cranpose_core::with_current_composer(|composer| composer.capture_composition_context());
    // Read while the composition is still running, same as `Layout`: measurement
    // happens after it and cannot reach a composition local. Reading here also
    // subscribes, so a subtree given a different grid recomposes and re-captures.
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        let modifier_changed = !node.modifier().structural_eq(&scroll_modifier);
        if refresh_content || config_changed || modifier_changed {
            node.set_modifier(scroll_modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        if refresh_content || config_changed || modifier_changed {
            measured_item_cache.borrow_mut().clear();
            node.request_measure_recompose();
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
    content: LazyListContentHandle,
) -> NodeId {
    cranpose_core::debug_label_current_scope("LazyColumnNode");

    // Expose this list as the nearest scroll container so a focused descendant
    // text field can scroll its caret above the soft keyboard. `report_window_rect`
    // fills `viewport` with the list's window rect each layout pass; the responder
    // maps a caret window rect to a scroll delta (pure `scroll_delta_to_reveal`).
    let viewport: Rc<Cell<cranpose_ui_graphics::Rect>> = cranpose_core::remember(|| {
        Rc::new(Cell::new(cranpose_ui_graphics::Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }))
    })
    .with(Rc::clone);
    let responder = {
        let viewport = Rc::clone(&viewport);
        cranpose_core::remember(move || {
            let viewport = Rc::clone(&viewport);
            crate::bring_into_view::BringIntoViewResponder::new(move |caret, ime_bottom| {
                let vp = viewport.get();
                if vp.width <= 0.0 || vp.height <= 0.0 {
                    return;
                }
                let delta = crate::bring_into_view::scroll_delta_to_reveal(caret, vp, ime_bottom);
                if delta.abs() > 0.5 {
                    // `scroll_delta_to_reveal` returns a positive delta to reveal
                    // content lower down (scroll forward); `LazyListState` scrolls
                    // forward on a NEGATIVE `dispatch_scroll_delta` (see
                    // `pushing_forward = delta < 0`), so negate.
                    state.dispatch_scroll_delta(-delta);
                }
            })
        })
        .with(|r| r.clone())
    };
    let modifier = modifier.report_window_rect(Rc::clone(&viewport));

    let mut node: Option<NodeId> = None;
    {
        let node_slot = &mut node;
        cranpose_core::CompositionLocalProvider(
            vec![
                crate::bring_into_view::local_bring_into_view_responder().provides(Some(responder)),
            ],
            move || {
                *node_slot = Some(LazyColumnImpl(modifier, state, spec, content));
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
    content: LazyListContentHandle,
) -> NodeId {
    cranpose_core::debug_label_current_scope("LazyRowNode");
    LazyRowImpl(modifier, state, spec, content)
}

/// A vertically scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyColumn` API. The closure receives
/// a [`LazyListIntervalContent`] which implements `LazyListScope` for defining items.
///
/// # Example
///
/// ```rust,ignore
/// let state = rememberLazyListState();
/// LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
///     // Single header item
///     scope.item_keyed(Some(0), None, || {
///         Text("Header", Modifier::empty());
///     });
///
///     // Multiple items from data
///     scope.items(data.len(), Some(|i| data[i].id), None, |i| {
///         Text(data[i].name.clone(), Modifier::empty());
///     });
/// });
/// ```
///
/// For convenience with slices, use the `LazyListScopeExt` extension methods:
///
/// ```rust,ignore
/// use cranpose_foundation::lazy::LazyListScopeExt;
///
/// LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
///     scope.items_slice(&my_data, |item| {
///         Text(item.name.clone(), Modifier::empty());
///     });
/// });
/// ```
/// A vertically scrolling list that only composes and lays out visible items.
///
/// # When to use
/// Use `LazyColumn` for lists with many items (100+) or unknown length.
/// It is much more efficient than using a `Column` with `vertical_scroll` modifier
/// because it recycles nodes and only keeps visible items in memory (virtualization).
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the list container.
/// * `state` - The scroll state, used to control scroll position or observe changes.
/// * `spec` - Configuration for content padding, item spacing, and reverse layout.
/// * `content` - A closure that defines the list content using `LazyListScope`.
///
/// # Example
///
/// ```rust,ignore
/// let state = rememberLazyListState();
/// LazyColumn(
///     Modifier::fill_max_size(),
///     state,
///     LazyColumnSpec::default(),
///     |scope| {
///         scope.items(1000, None, None, |i| {
///             Text(format!("Item {}", i), Modifier::padding(16.0));
///         });
///     }
/// );
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
    let mut interval_content = LazyListIntervalContent::new();
    content(&mut interval_content);
    LazyColumnNode(
        modifier,
        state,
        spec,
        LazyListContentHandle::new(interval_content),
    )
}

/// A horizontally scrolling list that only composes visible items.
///
/// Matches Jetpack Compose's `LazyRow` API. The closure receives
/// a [`LazyListIntervalContent`] which implements `LazyListScope` for defining items.
///
/// # Example
///
/// ```rust,ignore
/// let state = rememberLazyListState();
/// LazyRow(Modifier::empty(), state, LazyRowSpec::default(), |scope| {
///     scope.items(10, |i| {
///         Text(format!("Item {}", i), Modifier::empty());
///     });
/// });
/// ```
pub fn LazyRow<F>(modifier: Modifier, state: LazyListState, spec: LazyRowSpec, content: F) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let mut interval_content = LazyListIntervalContent::new();
    content(&mut interval_content);
    LazyRowNode(
        modifier,
        state,
        spec,
        LazyListContentHandle::new(interval_content),
    )
}

#[cfg(test)]
mod tests {
    use cranpose_core::{Composition, MemoryApplier, location_key};

    use super::*;

    #[test]
    fn test_lazy_column_spec_default() {
        let spec = LazyColumnSpec::default();
        assert_eq!(spec.vertical_arrangement, LinearArrangement::Start);
        assert_eq!(spec.beyond_bounds_item_count, 2);
    }

    #[test]
    fn test_lazy_column_spec_builder() {
        let spec = LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(8.0))
            .content_padding(16.0, 16.0);

        assert_eq!(spec.vertical_arrangement, LinearArrangement::SpacedBy(8.0));
        assert_eq!(spec.content_padding_top, 16.0);
    }

    #[test]
    fn test_lazy_row_spec_default() {
        let spec = LazyRowSpec::default();
        assert_eq!(spec.horizontal_arrangement, LinearArrangement::Start);
        assert_eq!(spec.beyond_bounds_item_count, 2);
    }

    #[test]
    fn test_get_spacing() {
        assert_eq!(get_spacing(LinearArrangement::Start), 0.0);
        assert_eq!(get_spacing(LinearArrangement::SpacedBy(12.0)), 12.0);
    }

    #[test]
    fn test_content_padding_all() {
        let spec = LazyColumnSpec::new().content_padding_all(24.0);
        assert_eq!(spec.content_padding_top, 24.0);
        assert_eq!(spec.content_padding_bottom, 24.0);
    }

    #[test]
    fn lazy_list_placements_reuse_output_storage() {
        let mut item = LazyListMeasuredItem::new(0, 10, None, 20.0, 50.0);
        item.offset = 7.0;
        item.node_ids.push(101);
        item.node_ids.push(102);
        item.child_offsets.push(0.0);
        item.child_offsets.push(5.0);
        let config = LazyListMeasureConfig {
            is_vertical: true,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 0.0,
            beyond_bounds_item_count: 0,
            vertical_arrangement: Some(LinearArrangement::Start),
            horizontal_arrangement: None,
        };
        let mut placements = Vec::with_capacity(8);
        let original_capacity = placements.capacity();

        push_lazy_list_placements(&mut placements, &[item], 1, true, 100.0, &config);

        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].node_id, 101);
        assert_eq!(placements[0].y, 7.0);
        assert_eq!(placements[1].node_id, 102);
        assert_eq!(placements[1].y, 12.0);
        assert_eq!(placements.capacity(), original_capacity);
    }

    #[test]
    fn lazy_list_placements_retain_offscreen_measured_items_for_renderer_prewarm() {
        let mut hidden = LazyListMeasuredItem::new(0, 10, None, 20.0, 50.0);
        hidden.offset = -40.0;
        hidden.node_ids.push(101);
        hidden.child_offsets.push(0.0);

        let mut partial = LazyListMeasuredItem::new(1, 11, None, 20.0, 50.0);
        partial.offset = -5.0;
        partial.node_ids.push(102);
        partial.child_offsets.push(0.0);

        let config = LazyListMeasureConfig {
            is_vertical: true,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 0.0,
            beyond_bounds_item_count: 2,
            vertical_arrangement: Some(LinearArrangement::Start),
            horizontal_arrangement: None,
        };
        let mut placements = Vec::new();

        push_lazy_list_placements(
            &mut placements,
            &[hidden, partial],
            100,
            true,
            100.0,
            &config,
        );

        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].node_id, 101);
        assert_eq!(placements[0].y, -40.0);
        assert_eq!(placements[1].node_id, 102);
        assert_eq!(placements[1].y, -5.0);
    }

    #[test]
    fn lazy_list_placements_retain_after_viewport_prefetch_items_for_renderer_prewarm() {
        let mut visible = LazyListMeasuredItem::new(0, 10, None, 40.0, 50.0);
        visible.offset = 60.0;
        visible.node_ids.push(101);
        visible.child_offsets.push(0.0);

        let mut warm = LazyListMeasuredItem::new(1, 11, None, 40.0, 50.0);
        warm.offset = 110.0;
        warm.node_ids.push(102);
        warm.child_offsets.push(0.0);

        let mut far = LazyListMeasuredItem::new(2, 12, None, 40.0, 50.0);
        far.offset = 158.0;
        far.node_ids.push(103);
        far.child_offsets.push(0.0);

        let config = LazyListMeasureConfig {
            is_vertical: true,
            reverse_layout: false,
            before_content_padding: 0.0,
            after_content_padding: 0.0,
            spacing: 8.0,
            beyond_bounds_item_count: 8,
            vertical_arrangement: Some(LinearArrangement::SpacedBy(8.0)),
            horizontal_arrangement: None,
        };
        let mut placements = Vec::new();

        push_lazy_list_placements(
            &mut placements,
            &[visible, warm, far],
            100,
            true,
            100.0,
            &config,
        );

        let placed_nodes = placements.iter().map(|p| p.node_id).collect::<Vec<_>>();
        assert_eq!(
            placed_nodes,
            vec![101, 102, 103],
            "prefetch rows remain in the retained placement list so renderers can prewarm clipped content"
        );
    }

    #[test]
    fn lazy_measure_policy_does_not_schedule_speculative_prefetch_frames() {
        let source = include_str!("lazy_list.rs");
        let start = source
            .find("fn measure_lazy_list_internal")
            .expect("measure function exists");
        let end = source[start..]
            .find("fn get_spacing")
            .map(|offset| start + offset)
            .expect("measure function boundary exists");
        let body = &source[start..end];

        assert!(
            !body.contains("prefetch_lazy_list_items")
                && !body.contains("schedule_layout_prewarm_repass"),
            "lazy layout measurement must not schedule speculative frame work"
        );
    }

    #[test]
    fn active_scroll_cached_reuse_validates_retained_children() {
        let source = include_str!("lazy_list.rs");
        let trust_mode = ["TrustClean", "RetainedScrollItem"].concat();
        let trust_api = ["trusting_", "cached_children"].concat();

        assert!(
            !source.contains(trust_mode.as_str()) && !source.contains(trust_api.as_str()),
            "lazy cached reuse must validate retained children during active scroll"
        );
    }

    #[test]
    fn lazy_list_state_identity_is_stable_for_copied_state() {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let mut state = None;
        composition
            .render(key, || {
                state = Some(cranpose_foundation::lazy::rememberLazyListState());
            })
            .expect("lazy list state render should succeed");
        let state = state.expect("lazy list state should be captured");
        let copied_state = state;

        assert_ne!(state.inner_ptr(), std::ptr::null());
        assert_eq!(
            lazy_list_state_identity(&state),
            lazy_list_state_identity(&copied_state)
        );
    }
}
