//! FlowRow widget implementation

use cranpose_core::NodeId;

use super::layout::Layout;
use crate::{composable, layout::policies::FlowRowMeasurePolicy, modifier::Modifier};

/// Specification for FlowRow layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowRowSpec {
    /// Horizontal gap between adjacent children on the same line, in dp
    /// (Compose: `horizontalArrangement = Arrangement.spacedBy(..)`).
    pub main_axis_spacing: f32,
    /// Vertical gap between consecutive lines, in dp
    /// (Compose: `verticalArrangement = Arrangement.spacedBy(..)`).
    pub cross_axis_spacing: f32,
}

impl FlowRowSpec {
    /// Creates a spec with no gaps between children or between rows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the horizontal gap between adjacent children on one row, in logical pixels.
    pub fn main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

    /// Sets the vertical gap between consecutive rows, in logical pixels.
    pub fn cross_axis_spacing(mut self, spacing: f32) -> Self {
        self.cross_axis_spacing = spacing;
        self
    }
}

impl Default for FlowRowSpec {
    fn default() -> Self {
        Self {
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
        }
    }
}

/// Places children across a row and starts a new row when the available width is full.
/// The spec sets the space between children and between rows.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{
///     widgets::{FlowRow, FlowRowSpec},
///     *,
/// };
///
/// #[composable]
/// fn Topics() {
///     FlowRow(
///         Modifier::empty().fill_max_width(),
///         FlowRowSpec::new()
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
pub fn FlowRow<F>(modifier: Modifier, spec: FlowRowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let mut policy = FlowRowMeasurePolicy::new(spec.main_axis_spacing, spec.cross_axis_spacing);
    policy.layout_direction = crate::layout_direction();
    Layout(modifier, policy, content)
}
