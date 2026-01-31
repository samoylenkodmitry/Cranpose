//! Column widget implementation

#![allow(non_snake_case)]

use super::layout::Layout;
use crate::composable;
use crate::layout::policies::FlexMeasurePolicy;
use crate::modifier::Modifier;
use cranpose_core::NodeId;
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

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

/// A layout composable that places its children in a vertical sequence.
///
/// # When to use
/// Use `Column` to arrange items top-to-bottom. For horizontal arrangement, use [`Row`](crate::widgets::Row).
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the column layout.
/// * `spec` - Configuration for vertical arrangement and horizontal alignment.
/// * `content` - The children composables to layout.
///
/// # Example
///
/// ```rust,ignore
/// Column(
///     Modifier::padding(16.0),
///     ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
///     || {
///         Text("Title", Modifier::empty());
///         Text("Subtitle", Modifier::empty());
///     }
/// );
/// ```
#[composable]
pub fn Column<F>(modifier: Modifier, spec: ColumnSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = FlexMeasurePolicy::column(spec.vertical_arrangement, spec.horizontal_alignment);
    Layout(modifier, policy, content)
}
