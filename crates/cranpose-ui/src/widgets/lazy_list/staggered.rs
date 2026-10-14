//! Lazy staggered grids: a [`LazyColumn`](super::LazyColumn) whose items go
//! into lanes of their own heights — Compose's
//! `LazyVerticalStaggeredGrid`. Each item takes the lane that ends first and
//! keeps it while the grid scrolls, or takes the whole line when its span
//! is the whole line.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::lazy::{
    BeyondItem, LazyItemSource as _, LazyListIntervalContent, LazyListMeasureResult,
    LazyListMeasuredItem, LazyListState, LazyStaggeredItemSource, StaggeredGridMeasureConfig,
    StaggeredLanes, measure_lazy_staggered_grid, rememberLazyListStateWithPosition,
};
use cranpose_ui_layout::{LinearArrangement, MeasureScope as _};

use super::{
    LazyColumnNode, LazyColumnSpec, LazyFrame, LazyListContentHandle, LazyListItemMeasureInputs,
    LazyListItems, LazyListMeasureContext, LazyRowNode, LazyRowSpec, PaddingValues,
    SubcomposeMeasureScopeImpl, beyond_composition, focused_lazy_item,
    grid::{CellLayout, CellSizes, GridCells, line_span_scope},
    lazy_list_child_constraints, request_beyond_items,
};
use crate::{composable, modifier::Modifier};

/// Options for a [`LazyVerticalStaggeredGrid`].
#[derive(Clone, Debug, PartialEq)]
pub struct LazyVerticalStaggeredGridSpec {
    pub columns: GridCells,
    /// The space between two items of a lane, in logical pixels.
    pub vertical_item_spacing: f32,
    /// The space between the lanes, in logical pixels.
    pub horizontal_spacing: f32,
    pub content_padding: PaddingValues,
    pub beyond_bounds_item_count: usize,
    pub reverse_layout: bool,
}

impl LazyVerticalStaggeredGridSpec {
    /// A grid of `columns` and no spacing.
    pub fn new(columns: GridCells) -> Self {
        Self {
            columns,
            vertical_item_spacing: 0.0,
            horizontal_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: LazyColumnSpec::default().beyond_bounds_item_count,
            reverse_layout: false,
        }
    }

    /// `spacing` logical pixels between the items and between the lanes.
    #[must_use]
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.vertical_item_spacing = spacing;
        self.horizontal_spacing = spacing;
        self
    }

    #[must_use]
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }
}

/// Options for a [`LazyHorizontalStaggeredGrid`].
#[derive(Clone, Debug, PartialEq)]
pub struct LazyHorizontalStaggeredGridSpec {
    pub rows: GridCells,
    /// The space between two items of a lane, in logical pixels.
    pub horizontal_item_spacing: f32,
    /// The space between the lanes, in logical pixels.
    pub vertical_spacing: f32,
    pub content_padding: PaddingValues,
    pub beyond_bounds_item_count: usize,
    pub reverse_layout: bool,
}

impl LazyHorizontalStaggeredGridSpec {
    /// A grid of `rows` and no spacing.
    pub fn new(rows: GridCells) -> Self {
        Self {
            rows,
            horizontal_item_spacing: 0.0,
            vertical_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: LazyRowSpec::default().beyond_bounds_item_count,
            reverse_layout: false,
        }
    }

    /// `spacing` logical pixels between the items and between the lanes.
    #[must_use]
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.horizontal_item_spacing = spacing;
        self.vertical_spacing = spacing;
        self
    }

    #[must_use]
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }
}

/// Where a lazy staggered grid is scrolled to — Compose's
/// `LazyStaggeredGridState`.
#[derive(Clone)]
pub struct LazyStaggeredGridState {
    list: LazyListState,
    lanes: Rc<RefCell<StaggeredLanes>>,
}

impl PartialEq for LazyStaggeredGridState {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list && Rc::ptr_eq(&self.lanes, &other.lanes)
    }
}

/// A [`LazyStaggeredGridState`] that starts at the top.
#[composable]
pub fn rememberLazyStaggeredGridState() -> LazyStaggeredGridState {
    let list = rememberLazyListStateWithPosition(0, 0.0);
    let lanes = cranpose_core::remember(|| Rc::new(RefCell::new(StaggeredLanes::default())))
        .with(Rc::clone);
    LazyStaggeredGridState { list, lanes }
}

impl LazyStaggeredGridState {
    /// The first item the grid shows, by index. A composable that reads it
    /// recomposes when it changes.
    pub fn first_visible_item_index(&self) -> usize {
        self.list.first_visible_item_index()
    }

    /// How far the viewport starts past the start of that item.
    pub fn first_visible_item_scroll_offset(&self) -> f32 {
        self.list.first_visible_item_scroll_offset()
    }

    /// Scrolls so that item `index` starts `offset` before the top.
    pub fn scroll_to_item(&self, index: usize, offset: f32) {
        self.list.scroll_to_item(index, offset);
    }

    /// The first visible item and how far the viewport starts past it, or
    /// `None` once the slot that held this state was released.
    pub fn try_position(&self) -> Option<(usize, f32)> {
        self.list.try_position()
    }

    /// A staggered grid's state with `index` first in view, `offset` past
    /// its start, for code that remembers the state itself. Call it inside a
    /// `remember`, as [`LazyListState::new`].
    pub fn new(index: usize, offset: f32) -> Self {
        Self {
            list: LazyListState::new(index, offset),
            lanes: Rc::new(RefCell::new(StaggeredLanes::default())),
        }
    }

    /// [`Self::scroll_to_item`] with a spring, which a press on the grid
    /// stops.
    pub fn animate_scroll_to_item(&self, index: usize, offset: f32) {
        crate::AnimateScroll::animate_scroll_to_item(&self.list, index, offset);
    }

    /// The state of the column the lanes sit in, for a scroll indicator.
    pub fn lazy_list_state(&self) -> LazyListState {
        self.list
    }
}

/// What a staggered grid measures with beside a list's configuration.
#[derive(Clone, Debug)]
pub(super) struct StaggeredLayout {
    cells: GridCells,
    spacing: f32,
    lanes: Rc<RefCell<StaggeredLanes>>,
}

impl PartialEq for StaggeredLayout {
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells
            && self.spacing == other.spacing
            && Rc::ptr_eq(&self.lanes, &other.lanes)
    }
}

/// The items of a staggered grid as its measure pass gets them: each one
/// as wide as its lane, or as the whole line.
struct StaggeredItems<'s, 'a, 'i> {
    items: LazyListItems<'s, 'a, 'i>,
    /// The item that holds the focus.
    focused: Option<usize>,
    lanes: usize,
    sizes: CellSizes,
    /// The width of the whole line.
    full: f32,
}

impl StaggeredItems<'_, '_, '_> {
    fn width(&self, lane: Option<usize>) -> f32 {
        lane.map_or(self.full, |lane| self.sizes.width(lane, 1))
    }
}

impl LazyStaggeredItemSource for StaggeredItems<'_, '_, '_> {
    fn is_full_line(&self, index: usize) -> bool {
        let lanes = self.lanes;
        self.items
            .inputs
            .content
            .with_interval(index, |local, interval| {
                interval
                    .span
                    .as_ref()
                    .is_some_and(|span| span(line_span_scope(lanes, lanes), local) >= lanes)
            })
            .unwrap_or(false)
    }

    fn measure(&mut self, index: usize, lane: Option<usize>) -> LazyListMeasuredItem {
        self.items.inputs.cross_axis_size = self.width(lane);
        self.items.measure(index)
    }

    fn keep_beyond(&mut self, index: usize, lane: Option<usize>, distance: f32) -> BeyondItem {
        let composed = self.items.composed(index);
        if !self.items.admits(composed, distance) {
            return BeyondItem::Declined;
        }
        self.items.inputs.cross_axis_size = self.width(lane);
        if self.items.place_beyond {
            return BeyondItem::Placed(self.items.measure(index));
        }
        self.items.keep(index, composed);
        BeyondItem::Kept
    }

    fn focused_item(&self) -> Option<usize> {
        self.focused
    }
}

/// Measures the items of a staggered grid's viewport, and returns them with
/// the number of items.
pub(super) fn measure_staggered_viewport(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    context: &LazyListMeasureContext<'_>,
    frame: &LazyFrame,
    layout: &StaggeredLayout,
) -> (LazyListMeasureResult, usize) {
    let state = context.state;
    let available = frame.item_cross_axis_size;
    let lanes = layout.cells.columns(available, layout.spacing);
    let sizes = CellSizes::new(available, lanes, layout.spacing, scope.density());
    context
        .measured_item_cache
        .borrow_mut()
        .begin_pass(scope, frame.is_vertical, available);
    let count = content.item_count();
    let near = state.nearest_range();
    if count > 0 {
        state.update_scroll_position_if_item_moved(count, |slot_id| {
            content.get_index_by_slot_id_in_range(slot_id, near.clone())
        });
    }
    let inputs = LazyListItemMeasureInputs {
        is_vertical: frame.is_vertical,
        cross_axis_size: available,
        content,
        state,
        measured_item_cache: context.measured_item_cache,
    };
    // While the grid holds the focus, the focused item stays placed and so
    // do the items beyond the viewport, where a focus search can move into
    // them.
    let focused = focused_lazy_item(scope, &inputs);
    let node_id = scope.root_id();
    let scroll_delta = state.peek_scroll_delta();
    let mut source = StaggeredItems {
        items: LazyListItems {
            scope: &mut *scope,
            inputs,
            near,
            scroll_delta,
            skipped_slots_recycled: false,
            place_beyond: focused.is_some(),
            beyond: None,
        },
        focused,
        lanes,
        sizes,
        full: available,
    };
    source.items.beyond = beyond_composition(&source.items);
    let grid = StaggeredGridMeasureConfig {
        lane_offsets: (0..lanes).map(|lane| sizes.offset(lane)).collect(),
    };
    let mut placed = layout.lanes.borrow_mut();
    let result = measure_lazy_staggered_grid(
        count,
        &mut placed,
        &grid,
        state,
        frame.raw_viewport_size,
        &context.config.measure,
        &mut source,
    );
    request_beyond_items(source.items.beyond, node_id);
    for item in &result.visible_items {
        let width = placed
            .lane_of(item.index)
            .map_or(available, |lane| sizes.width(lane, 1));
        scope.ensure_cached_measurement_node_ids(
            item.node_ids
                .iter()
                .filter_map(|&node_id| NodeId::try_from(node_id).ok()),
            lazy_list_child_constraints(frame.is_vertical, width),
        );
    }
    (result, count)
}

impl LazyStaggeredGridState {
    /// The layout a grid of `cells` with `spacing` between its lanes
    /// measures with.
    fn layout(&self, cells: GridCells, spacing: f32) -> Option<CellLayout> {
        Some(CellLayout::Staggered(StaggeredLayout {
            cells,
            spacing,
            lanes: Rc::clone(&self.lanes),
        }))
    }
}

/// Composes and measures the items needed for a vertical staggered grid's
/// viewport — Compose's `LazyVerticalStaggeredGrid`. `content` adds the
/// items as a [`LazyColumn`](super::LazyColumn)'s does; an item whose
/// [`LazyItems::span`](cranpose_foundation::lazy::LazyItems::span) is the
/// whole line takes the whole line.
pub fn LazyVerticalStaggeredGrid<F>(
    modifier: Modifier,
    state: LazyStaggeredGridState,
    spec: LazyVerticalStaggeredGridSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let column = LazyColumnSpec {
        vertical_arrangement: LinearArrangement::SpacedBy(spec.vertical_item_spacing),
        content_padding: spec.content_padding,
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        reverse_layout: spec.reverse_layout,
    };
    let layout = state.layout(spec.columns, spec.horizontal_spacing);
    LazyColumnNode(
        modifier,
        state.list,
        column,
        layout,
        LazyListContentHandle::build(content),
    )
}

/// Composes and measures the items needed for a horizontal staggered
/// grid's viewport, in rows of their own widths — Compose's
/// `LazyHorizontalStaggeredGrid`.
pub fn LazyHorizontalStaggeredGrid<F>(
    modifier: Modifier,
    state: LazyStaggeredGridState,
    spec: LazyHorizontalStaggeredGridSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let row = LazyRowSpec {
        horizontal_arrangement: LinearArrangement::SpacedBy(spec.horizontal_item_spacing),
        content_padding: spec.content_padding,
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        reverse_layout: spec.reverse_layout,
    };
    let layout = state.layout(spec.rows, spec.vertical_spacing);
    LazyRowNode(
        modifier,
        state.list,
        row,
        layout,
        LazyListContentHandle::build(content),
    )
}
