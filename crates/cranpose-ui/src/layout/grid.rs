//! The measure policy of `Grid`, after Compose's `GridMeasurePolicy`:
//! children get their cells, the tracks get their sizes, and children
//! measure in their cells and align there.

use cranpose_ui_graphics::whole;
use cranpose_ui_layout::{
    Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement, round_to_px,
};
use smallvec::SmallVec;

use crate::{
    layout::core::{Alignment, Measurable},
    widgets::grid::{GridConfig, GridFlow, GridItem, GridTrackSize, MAX_GRID_INDEX},
};

const MAX_INDEX: usize = MAX_GRID_INDEX as usize;

/// Lays out the children of a `Grid` by its config.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GridMeasurePolicy {
    pub(crate) config: GridConfig,
    pub(crate) layout_direction: crate::LayoutDirection,
    /// Device pixels per dp, which track sizes and gaps round to.
    pub(crate) density: f32,
}

/// A child's cell, from 0, and how it aligns there.
#[derive(Clone, Copy, Debug)]
struct Cell {
    child: usize,
    row: usize,
    column: usize,
    row_span: usize,
    column_span: usize,
    alignment: Alignment,
}

impl Cell {
    fn start(&self, axis: TrackAxis) -> usize {
        match axis {
            TrackAxis::Columns => self.column,
            TrackAxis::Rows => self.row,
        }
    }

    fn span(&self, axis: TrackAxis) -> usize {
        match axis {
            TrackAxis::Columns => self.column_span,
            TrackAxis::Rows => self.row_span,
        }
    }

    fn overlaps(&self, row: usize, column: usize, row_span: usize, column_span: usize) -> bool {
        row < self.row + self.row_span
            && self.row < row + row_span
            && column < self.column + self.column_span
            && self.column < column + column_span
    }
}

type Cells = SmallVec<[Cell; 16]>;
type Sizes = SmallVec<[f32; 8]>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackAxis {
    Columns,
    Rows,
}

/// The sizes of one axis' tracks and where each starts.
struct Tracks {
    sizes: Sizes,
    offsets: Sizes,
}

impl Tracks {
    fn new(sizes: Sizes, gap: f32) -> Self {
        let mut start = 0.0;
        let offsets = sizes
            .iter()
            .map(|size| {
                let offset = start;
                start += size + gap;
                offset
            })
            .collect();
        Self { sizes, offsets }
    }

    /// The length of all tracks with the gaps between them.
    fn total(&self) -> f32 {
        self.spanned(0, self.sizes.len())
    }

    /// The length of `span` tracks from `start`, with the gaps between them.
    fn spanned(&self, start: usize, span: usize) -> f32 {
        let end = (start + span).min(self.sizes.len());
        if start >= end {
            return 0.0;
        }
        self.offsets[end - 1] + self.sizes[end - 1] - self.offsets[start]
    }

    /// Where track `index` starts.
    fn offset(&self, index: usize) -> f32 {
        self.offsets.get(index).copied().unwrap_or_default()
    }
}

/// The grid's cells and the track counts they need, explicit and added.
struct Placed {
    cells: Cells,
    columns: usize,
    rows: usize,
}

impl GridMeasurePolicy {
    fn spec(&self, axis: TrackAxis, index: usize) -> GridTrackSize {
        let specs = match axis {
            TrackAxis::Columns => &self.config.columns,
            TrackAxis::Rows => &self.config.rows,
        };
        specs.get(index).copied().unwrap_or(GridTrackSize::Auto)
    }

    /// Gives every child a cell: named areas and explicit cells first, then
    /// the next free cell along the flow. Compose's `resolveGridItemIndices`.
    fn place_cells(&self, measurables: &[Box<dyn Measurable>]) -> Placed {
        let explicit_columns = self.config.columns.len();
        let explicit_rows = self.config.rows.len();
        let flow = self.config.flow;
        let mut cells = Cells::new();
        let (mut rows, mut columns) = (explicit_rows, explicit_columns);
        let (mut cursor_row, mut cursor_column) = (0, 0);

        for (child, measurable) in measurables.iter().enumerate() {
            let item = measurable
                .parent_data()
                .layout_data::<GridItem>()
                .copied()
                .unwrap_or_default();
            let (row, column, mut row_span, mut column_span) = self.requested(&item);
            if row == 0 && explicit_rows > 0 && flow == GridFlow::Column {
                row_span = row_span.min(explicit_rows);
            }
            if column == 0 && explicit_columns > 0 && flow == GridFlow::Row {
                column_span = column_span.min(explicit_columns);
            }
            let requested_row = zero_based(row, explicit_rows);
            let requested_column = zero_based(column, explicit_columns);
            let occupied = |row: usize, column: usize| {
                row + row_span > MAX_INDEX
                    || column + column_span > MAX_INDEX
                    || cells
                        .iter()
                        .any(|cell| cell.overlaps(row, column, row_span, column_span))
            };
            let (row, column) = match (requested_row, requested_column) {
                (Some(row), Some(column)) => (row, column),
                (Some(row), None) => {
                    let from = if flow == GridFlow::Row && row == cursor_row {
                        cursor_column
                    } else {
                        0
                    };
                    (row, first_free(from, |column| occupied(row, column)))
                }
                (None, Some(column)) => {
                    let from = if flow == GridFlow::Column && column == cursor_column {
                        cursor_row
                    } else {
                        0
                    };
                    (first_free(from, |row| occupied(row, column)), column)
                }
                (None, None) => {
                    let wrap_at = match flow {
                        GridFlow::Row => (explicit_columns, column_span),
                        GridFlow::Column => (explicit_rows, row_span),
                    };
                    let (line, along) = match flow {
                        GridFlow::Row => (cursor_row, cursor_column),
                        GridFlow::Column => (cursor_column, cursor_row),
                    };
                    let (line, along) =
                        next_free_slot(line, along, wrap_at, |line, along| match flow {
                            GridFlow::Row => occupied(line, along),
                            GridFlow::Column => occupied(along, line),
                        });
                    let (row, column) = match flow {
                        GridFlow::Row => (line, along),
                        GridFlow::Column => (along, line),
                    };
                    (cursor_row, cursor_column) = match flow {
                        GridFlow::Row => (row, column + column_span),
                        GridFlow::Column => (row + row_span, column),
                    };
                    (row, column)
                }
            };
            rows = rows.max(row + row_span);
            columns = columns.max(column + column_span);
            cells.push(Cell {
                child,
                row,
                column,
                row_span,
                column_span,
                alignment: item.alignment,
            });
        }
        // Row by row from the top start, which is also the order children
        // measure, place and draw in; a stable sort keeps children of one
        // cell in composition order.
        cells.sort_by_key(|cell| (cell.row, cell.column));
        Placed {
            cells,
            columns,
            rows,
        }
    }

    /// The row, column and spans a child asks for, from its area when it
    /// names a known one.
    fn requested(&self, item: &GridItem) -> (i32, i32, usize, usize) {
        let (row, column, row_span, column_span) = match item.area {
            Some(name) => self
                .config
                .areas
                .iter()
                .find(|area| area.name == name)
                .map_or((0, 0, 1, 1), |area| {
                    (area.row, area.column, area.row_span, area.column_span)
                }),
            None => (item.row, item.column, item.row_span, item.column_span),
        };
        (row, column, row_span as usize, column_span as usize)
    }

    /// The sizes of one axis' tracks with `available` room on that axis.
    /// For rows, `columns` holds the column sizes the children's heights
    /// depend on. Compose's `calculateColumnWidths`/`calculateRowHeights`.
    fn size_tracks(
        &self,
        axis: TrackAxis,
        placed: &Placed,
        measurables: &[Box<dyn Measurable>],
        columns: Option<&Tracks>,
        available: f32,
    ) -> Tracks {
        let density = self.density;
        let count = match axis {
            TrackAxis::Columns => placed.columns,
            TrackAxis::Rows => placed.rows,
        };
        let gap = round_to_px(
            match axis {
                TrackAxis::Columns => self.config.column_gap,
                TrackAxis::Rows => self.config.row_gap,
            },
            density,
        );
        let room = if available.is_finite() {
            (available - gap * count.saturating_sub(1) as f32).max(0.0)
        } else {
            f32::INFINITY
        };
        let intrinsic = |cell: &Cell, min: bool| {
            let measurable = measurables[cell.child].as_ref();
            match (axis, columns) {
                (TrackAxis::Rows, Some(columns)) => {
                    let width = columns.spanned(cell.column, cell.column_span);
                    if min {
                        measurable.min_intrinsic_height(width)
                    } else {
                        measurable.max_intrinsic_height(width)
                    }
                }
                _ if min => measurable.min_intrinsic_width(f32::INFINITY),
                _ => measurable.max_intrinsic_width(f32::INFINITY),
            }
        };

        // The largest smallest and largest unbounded content size of the
        // children in one track each, for the tracks that size to content.
        let mut content_min: Sizes = SmallVec::from_elem(0.0, count);
        let mut content_max: Sizes = SmallVec::from_elem(0.0, count);
        for cell in placed.cells.iter().filter(|cell| cell.span(axis) == 1) {
            let index = cell.start(axis);
            let (min, max) = content_queries(self.spec(axis, index), room.is_finite());
            if min {
                content_min[index] = content_min[index].max(intrinsic(cell, true));
            }
            if max {
                content_max[index] = content_max[index].max(intrinsic(cell, false));
            }
        }

        // Pass 1: base sizes.
        let mut sizes: Sizes = SmallVec::with_capacity(count);
        let mut total_flex = 0.0_f32;
        for index in 0..count {
            let size = match self.spec(axis, index) {
                GridTrackSize::Fixed(size) => round_to_px(size.max(0.0), density),
                GridTrackSize::Percentage(fraction) if room.is_finite() => {
                    round_to_px(fraction.max(0.0) * room, density)
                }
                GridTrackSize::Flex(weight) => {
                    total_flex += flex_weight(weight.0);
                    content_min[index]
                }
                GridTrackSize::MinMax(min, weight) => {
                    total_flex += flex_weight(weight.0);
                    round_to_px(min.max(0.0), density)
                }
                GridTrackSize::MinContent => content_min[index],
                GridTrackSize::Auto if room.is_finite() => content_min[index],
                GridTrackSize::Percentage(_) | GridTrackSize::MaxContent | GridTrackSize::Auto => {
                    content_max[index]
                }
            };
            sizes.push(size);
        }

        // Pass 1.5: tracks under a spanning child grow to fit it.
        for cell in placed.cells.iter().filter(|cell| cell.span(axis) > 1) {
            let start = cell.start(axis);
            let end = (start + cell.span(axis)).min(count);
            let current = sizes[start..end].iter().sum::<f32>()
                + gap * cell.span(axis).saturating_sub(1) as f32;
            let required = intrinsic(cell, false);
            let growable = (start..end)
                .filter(|&index| grows_for_spans(self.spec(axis, index)))
                .count();
            if growable == 0 {
                continue;
            }
            // Whole device pixels, rounded up so the child is not narrower
            // than it asked, split as evenly as they go.
            let deficit = whole::ceil((required - current) * density - 1e-3);
            if deficit <= 0.0 {
                continue;
            }
            let share = whole::floor(deficit / growable as f32);
            let mut remainder = deficit - share * growable as f32;
            for index in (start..end).filter(|&index| grows_for_spans(self.spec(axis, index))) {
                let extra = if remainder > 0.0 { 1.0 } else { 0.0 };
                remainder -= extra;
                sizes[index] += (share + extra) / density;
            }
        }

        if room.is_finite() {
            // Pass 1.8: auto tracks grow toward their content's unbounded size.
            grow_auto_tracks(
                &mut sizes,
                (0..count).filter(|&index| self.spec(axis, index) == GridTrackSize::Auto),
                &content_max,
                room,
                density,
            );
            // Pass 2: flexible tracks share what is left.
            let left = (room - sizes.iter().sum::<f32>()).max(0.0);
            if total_flex > 0.0 && left > 0.0 {
                let mut accumulated = 0.0_f32;
                let mut handed = 0.0_f32;
                for (index, size) in sizes.iter_mut().enumerate() {
                    let weight = match self.spec(axis, index) {
                        GridTrackSize::Flex(weight) | GridTrackSize::MinMax(_, weight) => {
                            flex_weight(weight.0)
                        }
                        _ => 0.0,
                    };
                    if weight <= 0.0 {
                        continue;
                    }
                    accumulated += weight;
                    // The last share ends exactly at the room's end.
                    let target = if accumulated >= total_flex {
                        left
                    } else {
                        round_to_px(accumulated / total_flex * left, density)
                    };
                    let share = (target - handed).max(0.0);
                    *size += share;
                    handed += share;
                }
            }
        }
        Tracks::new(sizes, gap)
    }

    /// The column and row sizes under `constraints`.
    fn size_grid(
        &self,
        placed: &Placed,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> (Tracks, Tracks) {
        let columns = self.size_tracks(
            TrackAxis::Columns,
            placed,
            measurables,
            None,
            constraints.max_width,
        );
        let rows = self.size_tracks(
            TrackAxis::Rows,
            placed,
            measurables,
            Some(&columns),
            constraints.max_height,
        );
        (columns, rows)
    }

    /// The grid's width with no bound on it, or its height when `width`
    /// bounds it: Compose measures a grid's intrinsic sizes this way, the
    /// smallest and the largest alike.
    fn intrinsic_width(&self, measurables: &[Box<dyn Measurable>]) -> f32 {
        let placed = self.place_cells(measurables);
        self.size_tracks(
            TrackAxis::Columns,
            &placed,
            measurables,
            None,
            f32::INFINITY,
        )
        .total()
    }

    fn intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        let placed = self.place_cells(measurables);
        let constraints = Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        let (_, rows) = self.size_grid(&placed, measurables, constraints);
        rows.total()
    }
}

/// Whether a track of `spec` needs its children's smallest content size,
/// and their unbounded one.
fn content_queries(spec: GridTrackSize, bounded: bool) -> (bool, bool) {
    match spec {
        GridTrackSize::Flex(_) | GridTrackSize::MinContent => (true, false),
        GridTrackSize::Auto if bounded => (true, true),
        GridTrackSize::Auto | GridTrackSize::MaxContent => (false, true),
        GridTrackSize::Percentage(_) => (false, !bounded),
        GridTrackSize::Fixed(_) | GridTrackSize::MinMax(..) => (false, false),
    }
}

/// Whether a track grows for children that span it; fixed and percentage
/// tracks keep their size.
fn grows_for_spans(spec: GridTrackSize) -> bool {
    !matches!(spec, GridTrackSize::Fixed(_) | GridTrackSize::Percentage(_))
}

fn flex_weight(weight: f32) -> f32 {
    if weight.is_finite() {
        weight.max(0.0)
    } else {
        0.0
    }
}

/// The first index from `from` that `occupied` leaves free; 0 when none
/// below the largest index is.
fn first_free(from: usize, occupied: impl Fn(usize) -> bool) -> usize {
    (from..MAX_INDEX)
        .find(|&index| !occupied(index))
        .unwrap_or(0)
}

/// The first free slot from `line` and `along` for a child that fills
/// lines one after another: along a line, wrapping to the next line when
/// the child no longer fits in `wrap_at.0` explicit tracks of its span
/// `wrap_at.1`. A child wider than all of them stays at a line's start
/// and adds tracks. `(0, 0)` when no slot is free.
fn next_free_slot(
    mut line: usize,
    mut along: usize,
    (explicit, span): (usize, usize),
    occupied: impl Fn(usize, usize) -> bool,
) -> (usize, usize) {
    while line < MAX_INDEX && along < MAX_INDEX {
        if explicit > 0 && along + span > explicit && along > 0 {
            along = 0;
            line += 1;
            continue;
        }
        if !occupied(line, along) {
            return (line, along);
        }
        along += 1;
        if along > MAX_INDEX {
            along = 0;
            line += 1;
        }
    }
    (0, 0)
}

/// Grows `auto` tracks from their smallest content size toward `content_max`
/// with the room their tracks leave, in proportion to how far each can
/// grow. Compose's `expandAutoTracks`.
fn grow_auto_tracks(
    sizes: &mut Sizes,
    auto: impl Iterator<Item = usize> + Clone,
    content_max: &Sizes,
    room: f32,
    density: f32,
) {
    let left = room - sizes.iter().sum::<f32>();
    if left <= 0.0 {
        return;
    }
    let potential = |index: usize| (content_max[index] - sizes[index]).max(0.0);
    let total: f32 = auto.clone().map(potential).sum();
    if total <= 0.0 {
        return;
    }
    let grow: SmallVec<[(usize, f32); 8]> = auto
        .map(|index| {
            let potential = potential(index);
            let extra = if left >= total {
                potential
            } else {
                round_to_px(potential / total * left, density)
            };
            (index, extra)
        })
        .collect();
    for (index, extra) in grow {
        sizes[index] += extra;
    }
}

/// A 1-based track index, negative from the last explicit track, as a
/// 0-based one. `None` when unspecified, or before the first track.
fn zero_based(index: i32, explicit: usize) -> Option<usize> {
    match index {
        0 => None,
        index if index > 0 => Some(index as usize - 1),
        index => explicit.checked_sub(index.unsigned_abs() as usize),
    }
}

impl MeasurePolicy for GridMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let density = self.density;
        let placed = self.place_cells(measurables);
        let (columns, rows) = self.size_grid(&placed, measurables, constraints);
        let (width, height) = constraints.constrain(columns.total(), rows.total());

        placements.reserve(placed.cells.len());
        for cell in &placed.cells {
            let cell_width = columns.spanned(cell.column, cell.column_span);
            let cell_height = rows.spanned(cell.row, cell.row_span);
            let placeable = measurables[cell.child].measure(Constraints {
                min_width: 0.0,
                max_width: cell_width,
                min_height: 0.0,
                max_height: cell_height,
            });
            let inside_x = cell
                .alignment
                .horizontal
                .align(cell_width, placeable.width(), density);
            let inside_x = self
                .layout_direction
                .place_x(inside_x, cell_width, placeable.width());
            let inside_y = cell
                .alignment
                .vertical
                .align(cell_height, placeable.height(), density);
            let cell_x =
                self.layout_direction
                    .place_x(columns.offset(cell.column), width, cell_width);
            let x = cell_x + inside_x;
            let y = rows.offset(cell.row) + inside_y;
            placeable.place(x, y);
            placements.push(Placement::new(placeable.node_id(), x, y, 0));
        }
        crate::modifier::Size { width, height }.into()
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_width(measurables)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_width(measurables)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.intrinsic_height(measurables, width)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.intrinsic_height(measurables, width)
    }
}
