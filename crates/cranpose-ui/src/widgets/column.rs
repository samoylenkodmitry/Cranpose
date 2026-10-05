//! Column widget implementation

use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

use super::layout::compose_layout;
use crate::{composable, layout::policies::FlexMeasurePolicy, modifier::Modifier};

/// Specification for Column layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnSpec {
    pub vertical_arrangement: LinearArrangement,
    pub horizontal_alignment: HorizontalAlignment,
}

impl ColumnSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vertical_arrangement(mut self, arrangement: LinearArrangement) -> Self {
        self.vertical_arrangement = arrangement;
        self
    }

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
#[composable]
pub fn Column<F>(modifier: Modifier, spec: ColumnSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let density = crate::density::density();
    let mut policy = FlexMeasurePolicy::column(
        spec.vertical_arrangement,
        spec.horizontal_alignment,
        density.density(),
    );
    policy.layout_direction = crate::layout_direction();
    compose_layout(modifier, policy, density, content)
}
