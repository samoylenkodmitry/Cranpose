//! Row widget implementation

use cranpose_core::NodeId;
use cranpose_ui_layout::{LinearArrangement, VerticalAlignment};

use super::layout::compose_layout;
use crate::{composable, layout::policies::FlexMeasurePolicy, modifier::Modifier};

/// Specification for Row layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowSpec {
    pub horizontal_arrangement: LinearArrangement,
    pub vertical_alignment: VerticalAlignment,
}

impl RowSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn horizontal_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.horizontal_arrangement = arrangement;
        self
    }

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
#[composable]
pub fn Row<F>(modifier: Modifier, spec: RowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let density = crate::density::density();
    let policy = FlexMeasurePolicy::row(
        spec.horizontal_arrangement,
        spec.vertical_alignment,
        density.density(),
    );
    compose_layout(modifier, policy, density, content)
}
