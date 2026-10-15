//! `FlowRow` and `FlowColumn`: children that wrap onto the next line.

use cranpose_core::NodeId;

use super::layout::Layout;
use crate::{
    composable,
    layout::policies::{FlowAxis, FlowMeasurePolicy},
    modifier::Modifier,
};

/// The gaps of a [`FlowRow`] or a [`FlowColumn`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlowSpec {
    /// Gap between adjacent children on the same line, in dp: horizontal in
    /// a row, vertical in a column (Compose: the arrangement along the main
    /// axis, `Arrangement.spacedBy(..)`).
    pub main_axis_spacing: f32,
    /// Gap between consecutive lines, in dp: between rows of a row, between
    /// columns of a column (Compose: the arrangement across them).
    pub cross_axis_spacing: f32,
}

impl FlowSpec {
    /// Creates a spec with no gaps between children or between lines.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the gap between adjacent children on one line, in logical pixels.
    pub fn main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the gap between consecutive lines, in logical pixels.
    pub fn cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }
}

/// Places children across a row and starts a new row when the available width is full.
/// The spec sets the space between children and between rows.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{
///     widgets::{FlowRow, FlowSpec},
///     *,
/// };
///
/// #[composable]
/// fn Topics() {
///     FlowRow(
///         Modifier::empty().fill_max_width(),
///         FlowSpec::new()
///             .main_axis_spacing(8.0)
///             .cross_axis_spacing(4.0),
///         || {
///             for label in ["Rust", "Compose", "Android"] {
///                 Text(label, Modifier::empty().padding(8.0), TextStyle::default());
///             }
///         },
///     );
/// }
/// ```
#[composable]
pub fn FlowRow<F>(modifier: Modifier, spec: FlowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    flow_layout(modifier, FlowAxis::Horizontal, spec, content)
}

/// Places children down a column and starts a new column when the
/// available height is full — Compose's `FlowColumn`. The spec sets the
/// space between children and between columns. In a right-to-left layout
/// the first column is at the right.
#[composable]
pub fn FlowColumn<F>(modifier: Modifier, spec: FlowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    flow_layout(modifier, FlowAxis::Vertical, spec, content)
}

fn flow_layout<F>(modifier: Modifier, axis: FlowAxis, spec: FlowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let mut policy = FlowMeasurePolicy::new(axis, spec.main_axis_spacing, spec.cross_axis_spacing);
    policy.layout_direction = crate::layout_direction();
    Layout(modifier, policy, content)
}
