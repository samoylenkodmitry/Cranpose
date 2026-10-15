//! `Grid`: children in rows and columns whose sizes the grid's tracks set.

use std::ops::RangeInclusive;

use cranpose_core::NodeId;
use smallvec::SmallVec;

use super::layout::compose_layout;
use crate::{
    composable,
    layout::{core::Alignment, grid::GridMeasurePolicy},
    modifier::Modifier,
};

/// The row or column index that leaves the choice to the grid. Compose's
/// `GridScope.GridIndexUnspecified`.
pub const GRID_INDEX_UNSPECIFIED: i32 = 0;

/// The largest row or column index and span; larger values count as this.
/// Compose's `GridScope.MaxGridIndex`.
pub const MAX_GRID_INDEX: i32 = 1000;

/// A share of the room a grid's other tracks leave. Compose's `Fr`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fr(pub f32);

/// The size of a row or a column of a [`Grid`]. Compose's `GridTrackSize`.
/// Negative sizes count as zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrackSize {
    /// A size in dp.
    Fixed(f32),
    /// A fraction, from 0 to 1, of the grid's room after its gaps. With no
    /// bound on the axis the track takes its content's size, as
    /// [`GridTrackSize::MaxContent`].
    Percentage(f32),
    /// A share of the room the other tracks leave, in proportion to the
    /// weight, and never smaller than its content's smallest size.
    Flex(Fr),
    /// At least the dp size, then a share of the room left as
    /// [`GridTrackSize::Flex`]. It never asks its content for a size, so a
    /// lazy list can sit in it.
    MinMax(f32, Fr),
    /// The smallest size the content takes.
    MinContent,
    /// The size the content takes with no limit.
    MaxContent,
    /// At least the content's smallest size, growing toward the size it
    /// takes with no limit while the grid has room.
    Auto,
}

/// The way children without a cell fill a [`Grid`]. Compose's `GridFlow`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GridFlow {
    /// Across the first row, then the next.
    #[default]
    Row,
    /// Down the first column, then the next.
    Column,
}

/// A named area of a grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridAreaDef {
    pub(crate) name: &'static str,
    pub(crate) row: i32,
    pub(crate) column: i32,
    pub(crate) row_span: u32,
    pub(crate) column_span: u32,
}

/// The tracks, gaps, flow and named areas of a [`Grid`]. Compose's
/// `GridConfigurationScope`: a track call adds the next row or column, and
/// the last gap call for an axis sets it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridConfig {
    pub(crate) columns: SmallVec<[GridTrackSize; 4]>,
    pub(crate) rows: SmallVec<[GridTrackSize; 4]>,
    pub(crate) row_gap: f32,
    pub(crate) column_gap: f32,
    pub(crate) flow: GridFlow,
    pub(crate) areas: SmallVec<[GridAreaDef; 2]>,
}

impl GridConfig {
    /// A grid with no explicit tracks or gaps, whose children fill rows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a column.
    pub fn column(mut self, size: GridTrackSize) -> Self {
        self.columns.push(size);
        self
    }

    /// Adds a column for each size, in order.
    pub fn columns(mut self, sizes: impl IntoIterator<Item = GridTrackSize>) -> Self {
        self.columns.extend(sizes);
        self
    }

    /// Adds a row.
    pub fn row(mut self, size: GridTrackSize) -> Self {
        self.rows.push(size);
        self
    }

    /// Adds a row for each size, in order.
    pub fn rows(mut self, sizes: impl IntoIterator<Item = GridTrackSize>) -> Self {
        self.rows.extend(sizes);
        self
    }

    /// Sets the gap between rows and between columns, in dp.
    pub fn gap(self, all: f32) -> Self {
        self.row_gap(all).column_gap(all)
    }

    /// Sets the gap between rows, in dp.
    pub fn row_gap(mut self, gap: f32) -> Self {
        self.row_gap = gap.max(0.0);
        self
    }

    /// Sets the gap between columns, in dp.
    pub fn column_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap.max(0.0);
        self
    }

    /// Sets the way children without a cell fill the grid.
    pub fn flow(mut self, flow: GridFlow) -> Self {
        self.flow = flow;
        self
    }

    /// Names an area that children take with [`GridItem::area`]. Rows and
    /// columns count from 1, and from the last explicit track when
    /// negative. With [`GRID_INDEX_UNSPECIFIED`] on one axis the area is
    /// that whole row or column, and its children fill it one after
    /// another. Naming an area again moves it.
    pub fn area(
        mut self,
        name: &'static str,
        row: i32,
        column: i32,
        row_span: u32,
        column_span: u32,
    ) -> Self {
        let area = GridAreaDef {
            name,
            row: clamp_index(row),
            column: clamp_index(column),
            row_span: clamp_span(row_span),
            column_span: clamp_span(column_span),
        };
        match self.areas.iter_mut().find(|known| known.name == name) {
            Some(known) => *known = area,
            None => self.areas.push(area),
        }
        self
    }

    /// Names the area of the rows and columns of two ranges:
    /// `area_ranges("footer", 2..=3, 1..=2)` is
    /// `area("footer", 2, 1, 2, 2)`.
    pub fn area_ranges(
        self,
        name: &'static str,
        rows: RangeInclusive<i32>,
        columns: RangeInclusive<i32>,
    ) -> Self {
        let (row, row_span) = start_and_span(rows);
        let (column, column_span) = start_and_span(columns);
        self.area(name, row, column, row_span, column_span)
    }
}

/// Where a child of a [`Grid`] sits and how it aligns in its cell. Compose's
/// `GridScope.gridItem` arguments; give it with [`Modifier::grid_item`].
/// A child without one takes the next free cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridItem {
    pub(crate) row: i32,
    pub(crate) column: i32,
    pub(crate) row_span: u32,
    pub(crate) column_span: u32,
    pub(crate) area: Option<&'static str>,
    pub(crate) alignment: Alignment,
}

impl GridItem {
    /// A child that takes the next free cell, at the top start of it.
    pub const fn auto() -> Self {
        Self {
            row: GRID_INDEX_UNSPECIFIED,
            column: GRID_INDEX_UNSPECIFIED,
            row_span: 1,
            column_span: 1,
            area: None,
            alignment: Alignment::TOP_START,
        }
    }

    /// A child in `row` and `column`, which count from 1, and from the
    /// last explicit track when negative (-1 is the last one). With
    /// [`GRID_INDEX_UNSPECIFIED`] on one axis the grid picks the first free
    /// cell along that axis. An index past the explicit tracks adds tracks
    /// sized [`GridTrackSize::Auto`].
    pub const fn at(row: i32, column: i32) -> Self {
        Self {
            row: clamp_index(row),
            column: clamp_index(column),
            ..Self::auto()
        }
    }

    /// A child over the rows and columns of two ranges:
    /// `GridItem::ranges(2..=3, 1..=2)` is `GridItem::at(2, 1).span(2, 2)`.
    pub fn ranges(rows: RangeInclusive<i32>, columns: RangeInclusive<i32>) -> Self {
        let (row, row_span) = start_and_span(rows);
        let (column, column_span) = start_and_span(columns);
        Self::at(row, column).span(row_span, column_span)
    }

    /// A child in the area [`GridConfig::area`] named `name`. A child of an
    /// unknown area takes the next free cell.
    pub const fn area(name: &'static str) -> Self {
        Self {
            area: Some(name),
            ..Self::auto()
        }
    }

    /// Sets how many rows and columns the child covers; a span counts as at
    /// least 1. A child the grid places itself spans no more columns than a
    /// row flow has, or rows than a column flow has.
    pub const fn span(mut self, rows: u32, columns: u32) -> Self {
        self.row_span = clamp_span(rows);
        self.column_span = clamp_span(columns);
        self
    }

    /// Sets where the child sits in its cell when it is smaller.
    pub const fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl Default for GridItem {
    fn default() -> Self {
        Self::auto()
    }
}

impl Modifier {
    /// Places this child of a [`Grid`]. Compose's `GridScope.gridItem`.
    pub fn grid_item(self, item: GridItem) -> Self {
        self.layout_data(item)
    }
}

const fn clamp_index(index: i32) -> i32 {
    if index > MAX_GRID_INDEX {
        MAX_GRID_INDEX
    } else if index < -MAX_GRID_INDEX {
        -MAX_GRID_INDEX
    } else {
        index
    }
}

const fn clamp_span(span: u32) -> u32 {
    if span == 0 {
        1
    } else if span > MAX_GRID_INDEX as u32 {
        MAX_GRID_INDEX as u32
    } else {
        span
    }
}

/// The first index of a range and the count of indices in it, at least 1.
fn start_and_span(range: RangeInclusive<i32>) -> (i32, u32) {
    let (start, end) = range.into_inner();
    (
        start,
        end.saturating_sub(start).saturating_add(1).max(1) as u32,
    )
}

/// Lays children out in rows and columns. The config sets the tracks, the
/// gaps and the named areas; each child takes the cell its
/// [`Modifier::grid_item`] names, or the next free one. Rows and columns
/// that children reach past the explicit tracks size to their content.
/// Compose's `Grid`.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{
///     widgets::{Fr, Grid, GridConfig, GridItem, GridTrackSize},
///     *,
/// };
///
/// #[composable]
/// fn Dashboard() {
///     Grid(
///         GridConfig::new()
///             .columns([GridTrackSize::Fixed(120.0), GridTrackSize::Flex(Fr(1.0))])
///             .gap(8.0),
///         Modifier::empty().fill_max_width(),
///         || {
///             Text(
///                 "Title",
///                 Modifier::empty().grid_item(GridItem::at(1, 1).span(1, 2)),
///                 TextStyle::default(),
///             );
///             Text("Name", Modifier::empty(), TextStyle::default());
///             Text("Ada", Modifier::empty(), TextStyle::default());
///         },
///     );
/// }
/// ```
#[composable]
pub fn Grid<F>(config: GridConfig, modifier: Modifier, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let density = crate::density::density();
    let policy = GridMeasurePolicy {
        config,
        layout_direction: crate::layout_direction(),
        density: density.density(),
    };
    compose_layout(modifier, policy, density, content)
}
