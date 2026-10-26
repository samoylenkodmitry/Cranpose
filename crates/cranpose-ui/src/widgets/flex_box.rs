//! `FlexBox`: children along a main axis that grow, shrink and wrap, as in
//! CSS flexbox.

use cranpose_core::NodeId;
use cranpose_ui_layout::Axis;

use super::layout::{LayoutEnvironment, compose_layout};
use crate::{composable, layout::flex_box::FlexBoxMeasurePolicy, modifier::Modifier};

/// The main axis of a [`FlexBox`]. Compose's `FlexDirection`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexDirection {
    /// Children from the start to the end of a row.
    #[default]
    Row,
    /// Children from the end to the start of a row.
    RowReverse,
    /// Children from the top to the bottom of a column.
    Column,
    /// Children from the bottom to the top of a column.
    ColumnReverse,
}

impl FlexDirection {
    pub(crate) fn main_axis(self) -> Axis {
        match self {
            Self::Row | Self::RowReverse => Axis::Horizontal,
            Self::Column | Self::ColumnReverse => Axis::Vertical,
        }
    }

    pub(crate) fn is_reverse(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
}

/// Whether a [`FlexBox`] starts new lines. Compose's `FlexWrap`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexWrap {
    /// All children on one line.
    #[default]
    NoWrap,
    /// New lines after the first, across the main axis.
    Wrap,
    /// New lines before the first: the last line is at the cross start.
    WrapReverse,
}

impl FlexWrap {
    pub(crate) fn wraps(self) -> bool {
        self != Self::NoWrap
    }
}

/// Where a line's children sit along the main axis when they leave room.
/// Compose's `FlexJustifyContent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexJustifyContent {
    /// At the main start.
    #[default]
    Start,
    /// At the main end.
    End,
    /// In the middle.
    Center,
    /// The first and last at the edges, the room between the others.
    SpaceBetween,
    /// Half as much room at the edges as between children.
    SpaceAround,
    /// As much room at the edges as between children.
    SpaceEvenly,
}

/// Where children sit across their line. Compose's `FlexAlignItems`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexAlignItems {
    /// At the cross start.
    #[default]
    Start,
    /// At the cross end.
    End,
    /// In the middle.
    Center,
    /// As big as the line across it.
    Stretch,
    /// Their first baselines on one line; a child without one aligns its
    /// bottom there.
    Baseline,
}

/// Where one child sits across its line, over the box's
/// [`FlexAlignItems`]. Compose's `FlexAlignSelf`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexAlignSelf {
    /// As the box's [`FlexAlignItems`] says.
    #[default]
    Auto,
    /// At the cross start.
    Start,
    /// At the cross end.
    End,
    /// In the middle.
    Center,
    /// As big as the line across it.
    Stretch,
    /// Its first baseline on the line's.
    Baseline,
}

/// Where the lines of a wrapping [`FlexBox`] sit across it when they leave
/// room. Compose's `FlexAlignContent`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexAlignContent {
    /// At the cross start.
    #[default]
    Start,
    /// At the cross end.
    End,
    /// In the middle.
    Center,
    /// Each line taller by an equal share of the room, up to the box's
    /// smallest size.
    Stretch,
    /// The first and last at the edges, the room between the others.
    SpaceBetween,
    /// Half as much room at the edges as between lines.
    SpaceAround,
}

/// A child's size along the main axis before it grows or shrinks. Compose's
/// `FlexBasis`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FlexBasis {
    /// The size the child takes with no limit.
    #[default]
    Auto,
    /// A size in dp.
    Dp(f32),
    /// A fraction, from 0 to 1, of the box's largest main size; the child's
    /// own size when that is unbounded.
    Percent(f32),
}

/// How a [`FlexBox`] lays its children out. Compose's `FlexBoxConfigScope`
/// settings; the last call for a setting wins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexBoxConfig {
    pub(crate) direction: FlexDirection,
    pub(crate) wrap: FlexWrap,
    pub(crate) justify_content: FlexJustifyContent,
    pub(crate) align_items: FlexAlignItems,
    pub(crate) align_content: FlexAlignContent,
    pub(crate) max_items_in_each_line: usize,
    pub(crate) row_gap: f32,
    pub(crate) column_gap: f32,
}

impl Default for FlexBoxConfig {
    fn default() -> Self {
        Self {
            direction: FlexDirection::Row,
            wrap: FlexWrap::NoWrap,
            justify_content: FlexJustifyContent::Start,
            align_items: FlexAlignItems::Start,
            align_content: FlexAlignContent::Start,
            max_items_in_each_line: usize::MAX,
            row_gap: 0.0,
            column_gap: 0.0,
        }
    }
}

impl FlexBoxConfig {
    /// A row that does not wrap, its children at the start and top.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the main axis.
    pub fn direction(mut self, direction: FlexDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets whether children wrap onto new lines.
    pub fn wrap(mut self, wrap: FlexWrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// Sets where a line's children sit along the main axis.
    pub fn justify_content(mut self, justify: FlexJustifyContent) -> Self {
        self.justify_content = justify;
        self
    }

    /// Sets where children sit across their line.
    pub fn align_items(mut self, align: FlexAlignItems) -> Self {
        self.align_items = align;
        self
    }

    /// Sets where the lines sit across a wrapping box.
    pub fn align_content(mut self, align: FlexAlignContent) -> Self {
        self.align_content = align;
        self
    }

    /// Sets how many children a line holds at most, at least 1; it applies
    /// when the box wraps.
    pub fn max_items_in_each_line(mut self, count: usize) -> Self {
        self.max_items_in_each_line = count.max(1);
        self
    }

    /// Sets the gap between rows, in dp: between the lines of a row box,
    /// between the children of a column box.
    pub fn row_gap(mut self, gap: f32) -> Self {
        self.row_gap = gap.max(0.0);
        self
    }

    /// Sets the gap between columns, in dp.
    pub fn column_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap.max(0.0);
        self
    }

    /// Sets both gaps, in dp.
    pub fn gap(self, all: f32) -> Self {
        self.row_gap(all).column_gap(all)
    }
}

/// How one child of a [`FlexBox`] sizes and aligns. Compose's
/// `FlexConfigScope` settings; give it with [`Modifier::flex`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlexConfig {
    pub(crate) align_self: FlexAlignSelf,
    pub(crate) order: i32,
    pub(crate) grow: Option<f32>,
    pub(crate) shrink: f32,
    pub(crate) basis: FlexBasis,
}

impl Default for FlexConfig {
    fn default() -> Self {
        Self {
            align_self: FlexAlignSelf::Auto,
            order: 0,
            grow: None,
            shrink: 1.0,
            basis: FlexBasis::Auto,
        }
    }
}

impl FlexConfig {
    /// A child that does not grow, shrinks with weight 1 and starts at the
    /// size it takes with no limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets where this child sits across its line.
    pub fn align_self(mut self, align: FlexAlignSelf) -> Self {
        self.align_self = align;
        self
    }

    /// Sets the child's place among its siblings: lower orders come first,
    /// equal ones keep the composition order.
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Sets the child's share of the room a line leaves; negative counts as
    /// 0. Without one, a child grows by the fraction its fill modifier gives
    /// along the main axis, and otherwise not.
    pub fn grow(mut self, grow: f32) -> Self {
        self.grow = Some(grow.max(0.0));
        self
    }

    /// Sets how much the child gives up, in proportion to its basis, when a
    /// line is too long; negative counts as 0.
    pub fn shrink(mut self, shrink: f32) -> Self {
        self.shrink = shrink.max(0.0);
        self
    }

    /// Sets the child's size before it grows or shrinks.
    pub fn basis(mut self, basis: FlexBasis) -> Self {
        self.basis = basis;
        self
    }
}

impl Modifier {
    /// Sets how this child of a [`FlexBox`] sizes and aligns. Compose's
    /// `FlexBoxScope.flex`.
    pub fn flex(self, config: FlexConfig) -> Self {
        self.layout_data(config)
    }
}

/// Lays children out along a main axis, as CSS flexbox does: they grow
/// into the room a line leaves, shrink when it is too short, and wrap onto
/// new lines when the config lets them. Compose's `FlexBox`.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{
///     widgets::{FlexBox, FlexBoxConfig, FlexConfig, FlexJustifyContent, FlexWrap},
///     *,
/// };
///
/// #[composable]
/// fn Tags() {
///     FlexBox(
///         Modifier::empty().fill_max_width(),
///         FlexBoxConfig::new()
///             .wrap(FlexWrap::Wrap)
///             .justify_content(FlexJustifyContent::SpaceBetween)
///             .gap(8.0),
///         || {
///             Text("Rust", Modifier::empty(), TextStyle::default());
///             Text(
///                 "Compose",
///                 Modifier::empty().flex(FlexConfig::new().grow(1.0)),
///                 TextStyle::default(),
///             );
///         },
///     );
/// }
/// ```
#[composable(no_skip)]
pub fn FlexBox<F>(modifier: Modifier, config: FlexBoxConfig, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let environment = LayoutEnvironment::current();
    let policy = FlexBoxMeasurePolicy {
        config,
        layout_direction: environment.direction,
        density: environment.density.density(),
    };
    compose_layout(modifier, policy, environment, content)
}
