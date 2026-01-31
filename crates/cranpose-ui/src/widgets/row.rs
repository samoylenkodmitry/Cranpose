//! Row widget implementation

#![allow(non_snake_case)]

use super::layout::Layout;
use crate::composable;
use crate::layout::policies::FlexMeasurePolicy;
use crate::modifier::Modifier;
use cranpose_core::NodeId;
use cranpose_ui_layout::{LinearArrangement, VerticalAlignment};

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
            vertical_alignment: VerticalAlignment::CenterVertically,
        }
    }
}

/// A layout composable that places its children in a horizontal sequence.
///
/// # When to use
/// Use `Row` to arrange items side-by-side. For vertical arrangement, use [`Column`](crate::widgets::Column).
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the row layout.
/// * `spec` - Configuration for horizontal arrangement and vertical alignment.
/// * `content` - The children composables to layout.
///
/// # Example
///
/// ```rust,ignore
/// Row(
///     Modifier::fill_max_width(),
///     RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
///     || {
///         Text("Left", Modifier::empty());
///         Text("Right", Modifier::empty());
///     }
/// );
/// ```
#[composable]
pub fn Row<F>(modifier: Modifier, spec: RowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = FlexMeasurePolicy::row(spec.horizontal_arrangement, spec.vertical_alignment);
    Layout(modifier, policy, content)
}
