//! Lazy grids: a [`LazyColumn`](super::LazyColumn) whose lines hold a row of
//! cells each. The list's measure pass runs over lines; each cell composes
//! and measures as a list item does, at the width of the columns it spans,
//! and a sticky header takes a whole line.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use cranpose_core::NodeId;
use cranpose_foundation::lazy::{
    BeyondItem, LazyGridItemSpanScope, LazyItemSource, LazyListIntervalContent,
    LazyListMeasureResult, LazyListMeasuredItem, LazyListState,
    measure_lazy_list_with_beyond_bounds_policy, rememberLazyListStateWithPosition,
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

    /// [`Self::scroll_to_item`] with a spring, which a press on the grid
    /// stops.
    pub fn animate_scroll_to_item(&self, index: usize, offset: f32) {
        let line = self.lines.borrow().line_of(index);
        crate::AnimateScroll::animate_scroll_to_item(&self.list, line, offset);
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

/// A run of lines.
#[derive(Clone, Copy, Debug)]
struct Run {
    first_line: usize,
    lines: usize,
    first_item: usize,
    items: usize,
    kind: RunKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RunKind {
    /// Whole lines of one-cell items.
    Cells,
    /// A sticky header's line.
    Header,
    /// Lines whose first items are listed from `GridLines::starts[from]`:
    /// the lines with spans, and a line a run of one-cell items leaves part
    /// full.
    Listed { from: usize },
}

/// The items of one line.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    first: usize,
    count: usize,
    header: bool,
}

/// An item of a line, the column it starts in and the columns it takes.
#[derive(Clone, Copy, Debug)]
struct Cell {
    item: usize,
    column: usize,
    span: usize,
}

/// How the items of a grid fall into lines. They are built once for a
/// content and a column count, and kept while the grid scrolls.
#[derive(Debug)]
pub(super) struct GridLines {
    columns: usize,
    runs: Vec<Run>,
    /// The first item of each line of the listed runs.
    starts: Vec<usize>,
    /// The lines of the sticky headers.
    headers: Vec<usize>,
    line_count: usize,
    /// The content the lines were built from. A weak pointer keeps its
    /// address from being reused by a later content.
    content: Weak<LazyListIntervalContent>,
}

impl Default for GridLines {
    fn default() -> Self {
        Self {
            columns: 1,
            runs: Vec::new(),
            starts: Vec::new(),
            headers: Vec::new(),
            line_count: 0,
            content: Weak::new(),
        }
    }
}

impl GridLines {
    fn new(content: &Rc<LazyListIntervalContent>, columns: usize) -> Self {
        let mut lines = Self {
            columns,
            content: Rc::downgrade(content),
            ..Self::default()
        };
        let mut builder = LineBuilder {
            lines: &mut lines,
            used: 0,
        };
        let mut headers = content.header_indices().iter().peekable();
        for interval in content.intervals() {
            if headers.next_if_eq(&&interval.start_index).is_some() {
                builder.header(interval.start_index);
            } else if let Some(span) = &interval.span {
                for local in 0..interval.count {
                    builder.spanned(interval.start_index + local, |left| {
                        span(line_span_scope(columns, left), local)
                    });
                }
            } else {
                builder.cells(interval.start_index, interval.count);
            }
        }
        lines
    }

    /// Whether these are the lines of `content` in `columns`.
    fn built_for(&self, content: &Rc<LazyListIntervalContent>, columns: usize) -> bool {
        self.columns == columns && std::ptr::eq(self.content.as_ptr(), Rc::as_ptr(content))
    }

    fn push_run(&mut self, kind: RunKind, first_item: usize, items: usize, lines: usize) {
        self.runs.push(Run {
            first_line: self.line_count,
            lines,
            first_item,
            items,
            kind,
        });
        self.line_count += lines;
    }

    /// The items of `line`; past the end, an empty line.
    fn line(&self, line: usize) -> Line {
        let at = self.runs.partition_point(|run| run.first_line <= line);
        let Some(run) = at
            .checked_sub(1)
            .and_then(|at| self.runs.get(at))
            .filter(|run| line < run.first_line + run.lines)
        else {
            return Line {
                first: self.runs.last().map_or(0, |run| run.first_item + run.items),
                count: 0,
                header: false,
            };
        };
        let in_run = line - run.first_line;
        match run.kind {
            RunKind::Header => Line {
                first: run.first_item,
                count: 1,
                header: true,
            },
            RunKind::Cells => Line {
                first: run.first_item + in_run * self.columns,
                count: self.columns,
                header: false,
            },
            RunKind::Listed { from } => {
                let first = self.starts[from + in_run];
                let end = if in_run + 1 < run.lines {
                    self.starts[from + in_run + 1]
                } else {
                    run.first_item + run.items
                };
                Line {
                    first,
                    count: end - first,
                    header: false,
                }
            }
        }
    }

    /// The line `item` sits in.
    fn line_of(&self, item: usize) -> usize {
        let at = self.runs.partition_point(|run| run.first_item <= item);
        at.checked_sub(1)
            .and_then(|at| self.runs.get(at))
            .map_or(item, |run| match run.kind {
                RunKind::Header => run.first_line,
                RunKind::Cells => run.first_line + (item - run.first_item) / self.columns,
                RunKind::Listed { from } => {
                    let starts = &self.starts[from..from + run.lines];
                    run.first_line
                        + starts
                            .partition_point(|&start| start <= item)
                            .saturating_sub(1)
                }
            })
    }

    /// The cells of `line`, whose spans `content` gives.
    fn cells<'c>(
        &self,
        line: Line,
        content: &'c LazyListIntervalContent,
    ) -> impl Iterator<Item = Cell> + use<'c> {
        let columns = self.columns;
        let mut column = 0;
        (line.first..line.first + line.count).map(move |item| {
            let left = columns.saturating_sub(column).max(1);
            let span = if line.header {
                columns
            } else {
                content
                    .with_interval(item, |local, interval| {
                        interval
                            .span
                            .as_ref()
                            .map(|span| span(line_span_scope(columns, left), local))
                    })
                    .flatten()
                    .map_or(1, |span| span.clamp(1, left))
            };
            let cell = Cell { item, column, span };
            column += span;
            cell
        })
    }
}

fn line_span_scope(columns: usize, left: usize) -> LazyGridItemSpanScope {
    LazyGridItemSpanScope {
        max_line_span: columns,
        max_current_line_span: left,
    }
}

/// Fills the lines of a grid item by item, as Compose's grid does: an item
/// that takes more cells than its line has left starts the next line.
struct LineBuilder<'l> {
    lines: &'l mut GridLines,
    /// The cells taken in the open line; none when no line is open.
    used: usize,
}

impl LineBuilder<'_> {
    fn header(&mut self, item: usize) {
        self.used = 0;
        let lines = &mut *self.lines;
        lines.headers.push(lines.line_count);
        lines.push_run(RunKind::Header, item, 1, 1);
    }

    fn cells(&mut self, mut first: usize, mut count: usize) {
        let columns = self.lines.columns;
        if self.used > 0 {
            let joined = count.min(columns - self.used);
            self.join(joined, joined);
            first += joined;
            count -= joined;
        }
        let whole = count / columns;
        if whole > 0 {
            self.lines
                .push_run(RunKind::Cells, first, whole * columns, whole);
            first += whole * columns;
            count -= whole * columns;
        }
        if count > 0 {
            self.open(first);
            self.join(count, count);
        }
    }

    /// Adds `item`, whose span `span_of` gives from the cells left in its
    /// line.
    fn spanned(&mut self, item: usize, span_of: impl Fn(usize) -> usize) {
        let columns = self.lines.columns;
        let mut span = span_of(columns - self.used).clamp(1, columns);
        if span > columns - self.used {
            self.used = 0;
            span = span_of(columns).clamp(1, columns);
        }
        if self.used == 0 {
            self.open(item);
        }
        self.join(1, span);
    }

    /// Starts a listed line at `item`.
    fn open(&mut self, item: usize) {
        let lines = &mut *self.lines;
        let listed = lines
            .runs
            .last()
            .is_some_and(|run| matches!(run.kind, RunKind::Listed { .. }));
        if !listed {
            let from = lines.starts.len();
            lines.push_run(RunKind::Listed { from }, item, 0, 0);
        }
        lines.starts.push(item);
        lines.line_count += 1;
        if let Some(run) = lines.runs.last_mut() {
            run.lines += 1;
        }
    }

    /// Adds `items` that take `cells` to the open line.
    fn join(&mut self, items: usize, cells: usize) {
        if let Some(run) = self.lines.runs.last_mut() {
            run.items += items;
        }
        self.used += cells;
        if self.used == self.lines.columns {
            self.used = 0;
        }
    }
}
/// The widths of a line's cells in whole device pixels, as Compose's grid
/// gives them: the pixels left over go one each to the first columns.
#[derive(Clone, Copy, Debug)]
struct CellSizes {
    /// The device pixels of the narrower columns.
    base: f32,
    /// How many columns, from the first, are one device pixel wider.
    wide: usize,
    /// The device pixels between two columns.
    spacing: f32,
    density: f32,
}

impl CellSizes {
    fn new(available: f32, columns: usize, spacing: f32, density: f32) -> Self {
        let density = density.max(f32::EPSILON);
        let spacing = (spacing * density).round();
        let cells = ((available * density).floor() - spacing * (columns - 1) as f32).max(0.0);
        let base = (cells / columns as f32).floor();
        Self {
            base,
            wide: (cells - base * columns as f32) as usize,
            spacing,
            density,
        }
    }

    /// Where `column` starts, in logical pixels.
    fn offset(&self, column: usize) -> f32 {
        (column as f32 * (self.base + self.spacing) + column.min(self.wide) as f32) / self.density
    }

    /// The width of a cell that takes `span` columns from `column`, with
    /// the spacing between them, in logical pixels.
    fn width(&self, column: usize, span: usize) -> f32 {
        let wide = (column + span).min(self.wide) - column.min(self.wide);
        (span as f32 * self.base + (span - 1) as f32 * self.spacing + wide as f32) / self.density
    }
}

/// The lines of a grid as a measure pass's items: each line measures its
/// cells as list items, each at its cell's width, and its header at the
/// full width.
struct LazyGridLines<'s, 'a, 'i> {
    items: LazyListItems<'s, 'a, 'i>,
    lines: &'i GridLines,
    sizes: CellSizes,
    /// The width of a header's line.
    full: f32,
}

impl LazyGridLines<'_, '_, '_> {
    /// Measures `item` at `width`.
    fn measure_at(&mut self, width: f32, item: usize) -> LazyListMeasuredItem {
        self.items.inputs.cross_axis_size = width;
        self.items.measure(item)
    }

    fn cell_width(&self, line: Line, cell: Cell) -> f32 {
        if line.header {
            self.full
        } else {
            self.sizes.width(cell.column, cell.span)
        }
    }
}

impl LazyItemSource for LazyGridLines<'_, '_, '_> {
    fn measure(&mut self, index: usize) -> LazyListMeasuredItem {
        let (lines, content) = (self.lines, self.items.inputs.content);
        let line = lines.line(index);
        if line.header {
            let mut header = self.measure_at(self.full, line.first);
            header.index = index;
            return header;
        }
        let key = content.get_key(line.first).to_slot_id();
        let mut measured = LazyListMeasuredItem::new(index, key, None, 0.0, 0.0);
        for cell in lines.cells(line, content) {
            let cross = self.sizes.offset(cell.column);
            let item = self.measure_at(self.cell_width(line, cell), cell.item);
            measured.main_axis_size = measured.main_axis_size.max(item.main_axis_size);
            measured.cross_axis_size = cross + item.cross_axis_size;
            for (root, &node_id) in item.node_ids.iter().enumerate() {
                let start = item.child_offsets.get(root).copied().unwrap_or(0.0);
                let end = item
                    .child_offsets
                    .get(root + 1)
                    .copied()
                    .unwrap_or(item.main_axis_size);
                measured.node_ids.push(node_id);
                measured.child_offsets.push(start);
                measured.child_cross_axis_sizes.push(
                    item.child_cross_axis_sizes
                        .get(root)
                        .copied()
                        .unwrap_or(item.cross_axis_size),
                );
                measured.child_cross_offsets.push(cross);
                measured.child_main_sizes.push(end - start);
            }
        }
        measured.main_axis_size = measured.main_axis_size.max(1.0);
        measured
    }

    fn keep_beyond(&mut self, index: usize, distance: f32) -> BeyondItem {
        let (lines, content) = (self.lines, self.items.inputs.content);
        let line = lines.line(index);
        for cell in lines.cells(line, content) {
            self.items.inputs.cross_axis_size = self.cell_width(line, cell);
            if let BeyondItem::Declined = self.items.keep_beyond(cell.item, distance) {
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
    content: &Rc<LazyListIntervalContent>,
    context: &LazyListMeasureContext<'_>,
    frame: &LazyFrame,
    grid: &GridLayout,
) -> (LazyListMeasureResult, usize) {
    let state = context.state;
    let available = frame.item_cross_axis_size;
    let columns = grid.cells.columns(available, grid.spacing);
    if !grid.lines.borrow().built_for(content, columns) {
        *grid.lines.borrow_mut() = GridLines::new(content, columns);
    }
    // A cell composed in this pass may read the grid's state, which reads
    // the lines.
    let lines = grid.lines.borrow();
    let sizes = CellSizes::new(available, columns, grid.spacing, scope.density());
    let line_count = lines.line_count;
    context
        .measured_item_cache
        .borrow_mut()
        .begin_pass(scope, frame.is_vertical, available);
    let near_lines = state.nearest_range();
    let last = lines.line(near_lines.end.saturating_sub(1));
    let near = lines.line(near_lines.start).first..last.first + last.count;
    if line_count > 0 {
        state.update_scroll_position_if_item_moved(line_count, |slot_id| {
            content
                .get_index_by_slot_id_in_range(slot_id, near.clone())
                .map(|item| lines.line_of(item))
        });
    }
    let inputs = LazyListItemMeasureInputs {
        is_vertical: frame.is_vertical,
        cross_axis_size: available,
        content,
        state,
        measured_item_cache: context.measured_item_cache,
    };
    let node_id = scope.root_id();
    let scroll_delta = state.peek_scroll_delta();
    let mut source = LazyGridLines {
        items: LazyListItems {
            scope: &mut *scope,
            inputs,
            near,
            scroll_delta,
            skipped_slots_recycled: false,
            place_beyond: false,
            beyond: None,
        },
        lines: &lines,
        sizes,
        full: available,
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
    if !lines.headers.is_empty() {
        pin_sticky_header(&mut result, &lines.headers, |line| source.measure(line));
    }
    register_grid_measurements(scope, &result, &lines, content, frame, sizes);
    (result, line_count)
}

/// Registers the measurements of the placed cells at their widths and of
/// the headers at the full width.
fn register_grid_measurements(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    result: &LazyListMeasureResult,
    lines: &GridLines,
    content: &LazyListIntervalContent,
    frame: &LazyFrame,
    sizes: CellSizes,
) {
    fn node_ids(ids: &[u64]) -> impl Iterator<Item = NodeId> + '_ {
        ids.iter()
            .filter_map(|&node_id| NodeId::try_from(node_id).ok())
    }
    for measured in &result.visible_items {
        let line = lines.line(measured.index);
        if line.header {
            scope.ensure_cached_measurement_node_ids(
                node_ids(&measured.node_ids),
                lazy_list_child_constraints(frame.is_vertical, frame.item_cross_axis_size),
            );
            continue;
        }
        let mut root = 0;
        for cell in lines.cells(line, content) {
            let cross = sizes.offset(cell.column);
            let roots = measured.child_cross_offsets[root..]
                .iter()
                .take_while(|&&offset| offset == cross)
                .count();
            scope.ensure_cached_measurement_node_ids(
                node_ids(&measured.node_ids[root..root + roots]),
                lazy_list_child_constraints(frame.is_vertical, sizes.width(cell.column, cell.span)),
            );
            root += roots;
        }
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
/// items as a [`LazyColumn`](super::LazyColumn)'s does, and
/// [`LazyItems::span`](cranpose_foundation::lazy::LazyItems::span) gives an
/// item more columns of its line. A sticky header takes a whole line and
/// holds the top while the lines under it scroll.
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
/// `LazyHorizontalGrid`. A span gives an item more rows of its column, and
/// a sticky header takes a whole column and holds the start edge.
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
