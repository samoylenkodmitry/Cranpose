//! Column widget implementation

use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

use super::layout::{LayoutEnvironment, compose_layout};
use crate::{composable, layout::policies::FlexMeasurePolicy, modifier::Modifier};

/// Specification for Column layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnSpec {
    /// How children are spaced along the vertical axis. Defaults to `Start`.
    pub vertical_arrangement: LinearArrangement,
    /// How children align on the horizontal axis. Defaults to `Start`.
    pub horizontal_alignment: HorizontalAlignment,
}

impl ColumnSpec {
    /// Creates a spec with start arrangement and start alignment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets how children are spaced along the vertical axis.
    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

    /// Sets how children align on the horizontal axis.
    pub fn horizontal_alignment(mut self, alignment: HorizontalAlignment) -> Self {
        self.horizontal_alignment = alignment;
        self
    }
}

impl Default for ColumnSpec {
    fn default() -> Self {
        Self {
            vertical_arrangement: LinearArrangement::Start,
            horizontal_alignment: HorizontalAlignment::Start,
        }
    }
}

/// Places children from top to bottom. `spec` sets vertical space and horizontal alignment.
/// Use [`Row`](crate::Row) for a horizontal sequence.
/// Like Compose's, it is inline: a state its content reads recomposes the
/// composable that called it.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Details() {
///     Column(
///         Modifier::empty().padding(16.0),
///         ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
///         || {
///             Text("Title", Modifier::empty(), TextStyle::default());
///             Text("Subtitle", Modifier::empty(), TextStyle::default());
///         },
///     );
/// }
/// ```
#[composable(no_skip)]
pub fn Column<F>(modifier: Modifier, spec: ColumnSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let environment = LayoutEnvironment::current();
    let mut policy = FlexMeasurePolicy::column(
        spec.vertical_arrangement,
        spec.horizontal_alignment,
        environment.density.density(),
    );
    policy.layout_direction = environment.direction;
    compose_layout(modifier, policy, environment, content)
}
