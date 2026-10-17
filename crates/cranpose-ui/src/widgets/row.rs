//! Row widget implementation

use cranpose_core::NodeId;
use cranpose_ui_layout::{LinearArrangement, VerticalAlignment};

use super::layout::{LayoutEnvironment, compose_layout};
use crate::{composable, layout::policies::FlexMeasurePolicy, modifier::Modifier};

/// Specification for Row layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSpec {
    /// How children are spaced along the horizontal axis. Defaults to `Start`.
    pub horizontal_arrangement: LinearArrangement,
    /// How children align on the vertical axis. Defaults to `Top`.
    pub vertical_alignment: VerticalAlignment,
}

impl RowSpec {
    /// Creates a spec with start arrangement and top alignment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets how children are spaced along the horizontal axis.
    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

    /// Sets how children align on the vertical axis.
    pub fn vertical_alignment(mut self, alignment: VerticalAlignment) -> Self {
        self.vertical_alignment = alignment;
        self
    }
}

impl Default for RowSpec {
    fn default() -> Self {
        Self {
            horizontal_arrangement: LinearArrangement::Start,
            vertical_alignment: VerticalAlignment::Top,
        }
    }
}

/// Places children from left to right, or right to left for an RTL layout.
/// `spec` sets horizontal space and vertical alignment.
/// Like Compose's, it is inline: a state its content reads recomposes the
/// composable that called it.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Summary() {
///     Row(
///         Modifier::empty().fill_max_width(),
///         RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
///         || {
///             Text("Name", Modifier::empty(), TextStyle::default());
///             Text("Value", Modifier::empty(), TextStyle::default());
///         },
///     );
/// }
/// ```
#[composable(no_skip)]
pub fn Row<F>(modifier: Modifier, spec: RowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let environment = LayoutEnvironment::current();
    let mut policy = FlexMeasurePolicy::row(
        spec.horizontal_arrangement,
        spec.vertical_alignment,
        environment.density.density(),
    );
    policy.layout_direction = environment.direction;
    compose_layout(modifier, policy, environment, content)
}
