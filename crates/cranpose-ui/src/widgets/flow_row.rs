//! FlowRow widget implementation

#![allow(non_snake_case)]

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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn main_axis_spacing(mut self, spacing: f32) -> Self {
        self.main_axis_spacing = spacing;
        self
    }

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

/// A layout composable that places its children in horizontal sequence and
/// wraps to the next line when it runs out of width (Jetpack Compose's
/// `FlowRow`).
///
/// # When to use
/// Use `FlowRow` for content whose item count or widths vary — chip groups,
/// tag clouds, toolbars on narrow screens. For a single non-wrapping line,
/// use [`Row`](crate::widgets::Row).
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the flow layout.
/// * `spec` - Spacing between items on a line and between lines.
/// * `content` - The children composables to layout.
///
/// # Example
///
/// ```rust,ignore
/// FlowRow(
///     Modifier::fill_max_width(),
///     FlowRowSpec::new().main_axis_spacing(8.0).cross_axis_spacing(4.0),
///     || {
///         for label in ["Rust", "Compose", "Android"] {
///             Chip(label);
///         }
///     },
/// );
/// ```
#[composable]
pub fn FlowRow<F>(modifier: Modifier, spec: FlowRowSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = FlowRowMeasurePolicy::new(spec.main_axis_spacing, spec.cross_axis_spacing);
    Layout(modifier, policy, content)
}
