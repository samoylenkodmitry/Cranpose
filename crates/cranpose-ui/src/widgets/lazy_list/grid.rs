//! Lazy grids: a [`LazyColumn`](super::LazyColumn) whose lines hold a row of
//! cells each. The list's measure pass runs over lines; each cell composes
//! and measures as a list item does, at the cell's width, and a sticky header
//! takes a whole line.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::lazy::{
    BeyondItem, LazyItemSource, LazyListIntervalContent, LazyListMeasureResult,
    LazyListMeasuredItem, LazyListState, measure_lazy_list_with_beyond_bounds_policy,
    rememberLazyListStateWithPosition,
};
use cranpose_ui_layout::{LinearArrangement, MeasureScope as _};

use super::{
    LazyColumnNode, LazyColumnSpec, LazyFrame, LazyListContentHandle, LazyListItemMeasureInputs,
    LazyListItems, LazyListMeasureContext, LazyRowNode, LazyRowSpec, PaddingValues,
    SubcomposeMeasureScopeImpl, beyond_composition, lazy_list_child_constraints, pin_sticky_header,
    request_beyond_items,
};
use crate::{composable, modifier::Modifier};

/// How a lazy grid splits its width into columns — Compose's `GridCells`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridCells {
    /// This many columns of equal width.
    Fixed(usize),
    /// As many columns as fit at least this many logical pixels wide, which
    /// share the width left over.
    Adaptive(f32),
}

impl GridCells {
    /// The number of columns in `available` width with `spacing` between them.
    fn columns(self, available: f32, spacing: f32) -> usize {
        match self {
            Self::Fixed(columns) => columns.max(1),
            Self::Adaptive(minimum) if minimum + spacing > 0.0 => {
                (((available + spacing) / (minimum + spacing)).floor() as usize).max(1)
            }
            Self::Adaptive(_) => 1,
        }
    }
}

/// Options for a [`LazyVerticalGrid`].
#[derive(Clone, Debug, PartialEq)]
pub struct LazyVerticalGridSpec {
    pub columns: GridCells,
    /// How the lines sit and the space between them.
    pub vertical_arrangement: LinearArrangement,
    /// The space between the cells of a line, in logical pixels.
    pub horizontal_spacing: f32,
    pub content_padding: PaddingValues,
    pub beyond_bounds_item_count: usize,
    pub reverse_layout: bool,
}

impl LazyVerticalGridSpec {
    /// A grid of `columns` and no spacing.
    pub fn new(columns: GridCells) -> Self {
        Self {
            columns,
            vertical_arrangement: LinearArrangement::Start,
            horizontal_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: LazyColumnSpec::default().beyond_bounds_item_count,
            reverse_layout: false,
        }
    }

    /// `spacing` logical pixels between the lines and between the cells.
    #[must_use]
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.vertical_arrangement = LinearArrangement::SpacedBy(spacing);
        self.horizontal_spacing = spacing;
        self
    }

    #[must_use]
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }
}

/// Options for a [`LazyHorizontalGrid`].
#[derive(Clone, Debug, PartialEq)]
pub struct LazyHorizontalGridSpec {
    pub rows: GridCells,
    /// How the lines sit and the space between them.
    pub horizontal_arrangement: LinearArrangement,
    /// The space between the cells of a line, in logical pixels.
    pub vertical_spacing: f32,
    pub content_padding: PaddingValues,
    pub beyond_bounds_item_count: usize,
    pub reverse_layout: bool,
}

impl LazyHorizontalGridSpec {
    /// A grid of `rows` and no spacing.
    pub fn new(rows: GridCells) -> Self {
        Self {
            rows,
            horizontal_arrangement: LinearArrangement::Start,
            vertical_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_bounds_item_count: LazyRowSpec::default().beyond_bounds_item_count,
            reverse_layout: false,
        }
    }

    /// `spacing` logical pixels between the lines and between the cells.
    #[must_use]
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.horizontal_arrangement = LinearArrangement::SpacedBy(spacing);
        self.vertical_spacing = spacing;
        self
    }

    #[must_use]
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }
}

/// Where a lazy grid is scrolled to, in items — Compose's `LazyGridState`.
#[derive(Clone)]
pub struct LazyGridState {
    list: LazyListState,
    lines: Rc<RefCell<GridLines>>,
}

impl PartialEq for LazyGridState {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list && Rc::ptr_eq(&self.lines, &other.lines)
    }
}

/// A [`LazyGridState`] that starts at the top.
#[composable]
pub fn rememberLazyGridState() -> LazyGridState {
    let list = rememberLazyListStateWithPosition(0, 0.0);
    let lines =
        cranpose_core::remember(|| Rc::new(RefCell::new(GridLines::default()))).with(Rc::clone);
    LazyGridState { list, lines }
}

impl LazyGridState {
    /// The first item of the first line the grid shows. A composable that
    /// reads it recomposes when it changes.
    pub fn first_visible_item_index(&self) -> usize {
        let line = self.list.first_visible_item_index();
        self.lines.borrow().line(line).first
    }

    /// How far the first visible line is scrolled past the top.
    pub fn first_visible_item_scroll_offset(&self) -> f32 {
        self.list.first_visible_item_scroll_offset()
    }

    /// Scrolls the line of `index` to the top, moved `offset` further.
    pub fn scroll_to_item(&self, index: usize, offset: f32) {
        let line = self.lines.borrow().line_of(index);
        self.list.scroll_to_item(line, offset);
    }

    /// The state of the column the lines sit in, for a scroll indicator.
    pub fn lazy_list_state(&self) -> LazyListState {
        self.list
    }
}

/// What a grid measures with beside a list's configuration.
#[derive(Clone, Debug)]
pub(super) struct GridLayout {
    cells: GridCells,
    spacing: f32,
    /// The latest lines, which the grid's state maps its items with.
    lines: Rc<RefCell<GridLines>>,
}

impl PartialEq for GridLayout {
    fn eq(&self, other: &Self) -> bool {
        self.cells == other.cells
            && self.spacing == other.spacing
            && Rc::ptr_eq(&self.lines, &other.lines)
    }
}

/// A run of items: the cells of whole lines, or one sticky header line.
#[derive(Clone, Copy, Debug)]
struct Run {
    first_line: usize,
    first_item: usize,
    items: usize,
    header: bool,
}

/// The items of one line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    first: usize,
    count: usize,
    header: bool,
}

/// How the items of a grid fall into lines.
#[derive(Clone, Debug)]
pub(super) struct GridLines {
    columns: usize,
    runs: Vec<Run>,
    line_count: usize,
}

impl Default for GridLines {
    fn default() -> Self {
        Self {
            columns: 1,
            runs: Vec::new(),
            line_count: 0,
        }
    }
}

impl GridLines {
    fn new(content: &LazyListIntervalContent, columns: usize) -> Self {
        let mut lines = Self {
            columns,
            runs: Vec::with_capacity(content.header_indices().len() * 2 + 1),
            line_count: 0,
        };
        let mut next = 0;
        for &header in content.header_indices() {
            lines.push_cells(next, header);
            lines.runs.push(Run {
                first_line: lines.line_count,
                first_item: header,
                items: 1,
                header: true,
            });
            lines.line_count += 1;
            next = header + 1;
        }
        lines.push_cells(next, content.item_count());
        lines
    }

    fn push_cells(&mut self, first: usize, end: usize) {
        if end <= first {
            return;
        }
        self.runs.push(Run {
            first_line: self.line_count,
            first_item: first,
            items: end - first,
            header: false,
        });
        self.line_count += (end - first).div_ceil(self.columns);
    }

    /// The items of `line`; past the end, an empty line.
    fn line(&self, line: usize) -> Line {
        let at = self.runs.partition_point(|run| run.first_line <= line);
        let Some(run) = at.checked_sub(1).and_then(|at| self.runs.get(at)) else {
            return Line {
                first: line,
                count: 0,
                header: false,
            };
        };
        if run.header {
            return Line {
                first: run.first_item,
                count: 1,
                header: true,
            };
        }
        let skipped = (line - run.first_line) * self.columns;
        Line {
            first: run.first_item + skipped,
            count: self.columns.min(run.items.saturating_sub(skipped)),
            header: false,
        }
    }

    /// The line `item` sits in.
    fn line_of(&self, item: usize) -> usize {
        let at = self.runs.partition_point(|run| run.first_item <= item);
        at.checked_sub(1)
            .and_then(|at| self.runs.get(at))
            .map_or(item, |run| {
                if run.header {
                    run.first_line
                } else {
                    run.first_line + (item - run.first_item) / self.columns
                }
            })
    }

    fn header_lines(&self) -> Vec<usize> {
        self.runs
            .iter()
            .filter(|run| run.header)
            .map(|run| run.first_line)
            .collect()
    }
}

/// The widths of a line's cells in whole device pixels, as Compose's grid
/// gives them: the pixels left over go one each to the first cells.
#[derive(Clone, Copy, Debug)]
struct CellSizes {
    /// The width of the narrower cells, in logical pixels.
    base: f32,
    /// How many cells, from the first, are one device pixel wider.
    wide: usize,
    /// One device pixel, in logical pixels.
    pixel: f32,
    spacing: f32,
}

impl CellSizes {
    fn new(available: f32, columns: usize, spacing: f32, density: f32) -> Self {
        let density = density.max(f32::EPSILON);
        let spacing_px = (spacing * density).round();
        let cells_px = ((available * density).floor() - spacing_px * (columns - 1) as f32).max(0.0);
        let base_px = (cells_px / columns as f32).floor();
        Self {
            base: base_px / density,
            wide: (cells_px - base_px * columns as f32) as usize,
            pixel: 1.0 / density,
            spacing: spacing_px / density,
        }
    }

    fn width(&self, column: usize) -> f32 {
        if column < self.wide {
            self.base + self.pixel
        } else {
            self.base
        }
    }

    fn offset(&self, column: usize) -> f32 {
        column as f32 * (self.base + self.spacing) + column.min(self.wide) as f32 * self.pixel
    }
}

/// The lines of a grid as a measure pass's items: each line measures its
/// cells as list items, each at its cell's width, and its header at the
/// full width.
struct LazyGridLines<'s, 'a, 'i> {
    items: LazyListItems<'s, 'a, 'i>,
    /// The inputs of the wider and of the narrower cells.
    cells: [&'i LazyListItemMeasureInputs<'i>; 2],
    headers: &'i LazyListItemMeasureInputs<'i>,
    lines: &'i GridLines,
    sizes: CellSizes,
}

impl<'i> LazyGridLines<'_, '_, 'i> {
    /// Measures `item` with the `inputs` of a header, a wide or a narrow cell.
    fn measure_with(&mut self, inputs: usize, item: usize) -> LazyListMeasuredItem {
        let inputs = self.inputs(inputs);
        let held = std::mem::replace(&mut self.items.inputs, inputs);
        let measured = self.items.measure(item);
        self.items.inputs = held;
        measured
    }

    fn keep_with(&mut self, inputs: usize, item: usize, distance: f32) -> BeyondItem {
        let inputs = self.inputs(inputs);
        let held = std::mem::replace(&mut self.items.inputs, inputs);
        let kept = self.items.keep_beyond(item, distance);
        self.items.inputs = held;
        kept
    }

    /// The inputs of `HEADER`, of a wide cell or of a narrow one.
    fn inputs(&self, inputs: usize) -> &'i LazyListItemMeasureInputs<'i> {
        if inputs == HEADER {
            self.headers
        } else {
            self.cells[inputs]
        }
    }

    fn cell_inputs(&self, column: usize) -> usize {
        usize::from(column >= self.sizes.wide)
    }
}

/// Which inputs measure a header line.
const HEADER: usize = 2;

impl LazyItemSource for LazyGridLines<'_, '_, '_> {
    fn measure(&mut self, index: usize) -> LazyListMeasuredItem {
        let line = self.lines.line(index);
        if line.header {
            let mut header = self.measure_with(HEADER, line.first);
            header.index = index;
            return header;
        }
        let key = self.items.inputs.content.get_key(line.first).to_slot_id();
        let mut measured = LazyListMeasuredItem::new(index, key, None, 0.0, 0.0);
        for column in 0..line.count {
            let cell = self.measure_with(self.cell_inputs(column), line.first + column);
            let cross = self.sizes.offset(column);
            measured.main_axis_size = measured.main_axis_size.max(cell.main_axis_size);
            measured.cross_axis_size = cross + cell.cross_axis_size;
            for (root, &node_id) in cell.node_ids.iter().enumerate() {
                measured.node_ids.push(node_id);
                measured
                    .child_offsets
                    .push(cell.child_offsets.get(root).copied().unwrap_or(0.0));
                measured.child_cross_axis_sizes.push(
                    cell.child_cross_axis_sizes
                        .get(root)
                        .copied()
                        .unwrap_or(cell.cross_axis_size),
                );
                measured.child_cross_offsets.push(cross);
                let start = cell.child_offsets.get(root).copied().unwrap_or(0.0);
                let end = cell
                    .child_offsets
                    .get(root + 1)
                    .copied()
                    .unwrap_or(cell.main_axis_size);
                measured.child_main_sizes.push(end - start);
            }
        }
        measured.main_axis_size = measured.main_axis_size.max(1.0);
        measured
    }

    fn keep_beyond(&mut self, index: usize, distance: f32) -> BeyondItem {
        let line = self.lines.line(index);
        for column in 0..line.count {
            let inputs = if line.header {
                HEADER
            } else {
                self.cell_inputs(column)
            };
            if let BeyondItem::Declined = self.keep_with(inputs, line.first + column, distance) {
                return BeyondItem::Declined;
            }
        }
        BeyondItem::Kept
    }
}

/// Measures the lines of a grid's viewport and holds its sticky header, and
/// returns them with the number of lines.
pub(super) fn measure_grid_viewport(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    content: &LazyListIntervalContent,
    context: &LazyListMeasureContext<'_>,
    frame: &LazyFrame,
    grid: &GridLayout,
) -> (LazyListMeasureResult, usize) {
    let state = context.state;
    let available = frame.item_cross_axis_size;
    let columns = grid.cells.columns(available, grid.spacing);
    let sizes = CellSizes::new(available, columns, grid.spacing, scope.density());
    let lines = GridLines::new(content, columns);
    let line_count = lines.line_count;
    context
        .measured_item_cache
        .borrow_mut()
        .begin_pass(scope, frame.is_vertical, sizes.base);
    let near_lines = state.nearest_range();
    let near = lines.line(near_lines.start).first
        ..lines
            .line(near_lines.end.saturating_sub(1))
            .first
            .saturating_add(columns);
    if line_count > 0 {
        state.update_scroll_position_if_item_moved(line_count, |slot_id| {
            content
                .get_index_by_slot_id_in_range(slot_id, near.clone())
                .map(|item| lines.line_of(item))
        });
    }
    let narrow = LazyListItemMeasureInputs {
        is_vertical: frame.is_vertical,
        cross_axis_size: sizes.base,
        content,
        state,
        measured_item_cache: context.measured_item_cache,
    };
    let wide = LazyListItemMeasureInputs {
        cross_axis_size: sizes.width(0),
        ..narrow
    };
    let headers = LazyListItemMeasureInputs {
        cross_axis_size: available,
        ..narrow
    };
    let node_id = scope.root_id();
    let scroll_delta = state.peek_scroll_delta();
    let mut source = LazyGridLines {
        items: LazyListItems {
            scope: &mut *scope,
            inputs: &narrow,
            near,
            scroll_delta,
            skipped_slots_recycled: false,
            place_beyond: false,
            beyond: None,
        },
        cells: [&wide, &narrow],
        headers: &headers,
        lines: &lines,
        sizes,
    };
    source.items.beyond = beyond_composition(&source.items);
    let mut result = measure_lazy_list_with_beyond_bounds_policy(
        line_count,
        state,
        frame.raw_viewport_size,
        available,
        &context.config.measure,
        &mut source,
    );
    request_beyond_items(source.items.beyond, node_id);
    if !content.header_indices().is_empty() {
        pin_sticky_header(&mut result, &lines.header_lines(), |line| {
            source.measure(line)
        });
    }
    register_grid_measurements(scope, &result, &lines, frame, sizes);
    *grid.lines.borrow_mut() = lines;
    (result, line_count)
}

/// Registers the measurements of the placed cells at their widths and of
/// the headers at the full width.
fn register_grid_measurements(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    result: &LazyListMeasureResult,
    lines: &GridLines,
    frame: &LazyFrame,
    sizes: CellSizes,
) {
    let first_narrow = sizes.offset(sizes.wide);
    let registrations = [
        (false, true, sizes.width(0)),
        (false, false, sizes.base),
        (true, false, frame.item_cross_axis_size),
    ];
    for (header, wide, width) in registrations {
        scope.ensure_cached_measurement_node_ids(
            result
                .visible_items
                .iter()
                .filter(|line| lines.line(line.index).header == header)
                .flat_map(|line| {
                    line.node_ids
                        .iter()
                        .enumerate()
                        .filter_map(move |(root, &node_id)| {
                            let offset = line.child_cross_offsets.get(root).copied();
                            (header || offset.is_some_and(|offset| (offset < first_narrow) == wide))
                                .then_some(node_id)
                        })
                })
                .filter_map(|node_id| NodeId::try_from(node_id).ok()),
            lazy_list_child_constraints(frame.is_vertical, width),
        );
    }
}

/// The content and the grid layout a grid's lazy column or row gets.
fn grid_parts<F>(
    state: &LazyGridState,
    cells: GridCells,
    spacing: f32,
    content: F,
) -> (Option<GridLayout>, LazyListContentHandle)
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let mut interval_content = LazyListIntervalContent::new();
    content(&mut interval_content);
    let grid = GridLayout {
        cells,
        spacing,
        lines: Rc::clone(&state.lines),
    };
    (Some(grid), LazyListContentHandle::new(interval_content))
}

/// Composes and measures the items needed for a vertical grid's viewport,
/// in lines of cells — Compose's `LazyVerticalGrid`. `content` adds the
/// items as a [`LazyColumn`](super::LazyColumn)'s does; a sticky header takes
/// a whole line and holds the top while the lines under it scroll.
pub fn LazyVerticalGrid<F>(
    modifier: Modifier,
    state: LazyGridState,
    spec: LazyVerticalGridSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let column = LazyColumnSpec {
        vertical_arrangement: spec.vertical_arrangement,
        content_padding: spec.content_padding,
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        reverse_layout: spec.reverse_layout,
    };
    let (grid, content) = grid_parts(&state, spec.columns, spec.horizontal_spacing, content);
    LazyColumnNode(modifier, state.list, column, grid, content)
}

/// Composes and measures the items needed for a horizontal grid's
/// viewport, in columns of cells from top to bottom — Compose's
/// `LazyHorizontalGrid`. A sticky header takes a whole column and holds the
/// start edge.
pub fn LazyHorizontalGrid<F>(
    modifier: Modifier,
    state: LazyGridState,
    spec: LazyHorizontalGridSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut LazyListIntervalContent),
{
    let row = LazyRowSpec {
        horizontal_arrangement: spec.horizontal_arrangement,
        content_padding: spec.content_padding,
        beyond_bounds_item_count: spec.beyond_bounds_item_count,
        reverse_layout: spec.reverse_layout,
    };
    let (grid, content) = grid_parts(&state, spec.rows, spec.vertical_spacing, content);
    LazyRowNode(modifier, state.list, row, grid, content)
}
