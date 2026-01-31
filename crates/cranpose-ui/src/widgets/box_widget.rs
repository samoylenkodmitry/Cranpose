//! Box widget implementation

#![allow(non_snake_case)]

use super::layout::Layout;
use crate::composable;
use crate::layout::policies::BoxMeasurePolicy;
use crate::modifier::Modifier;
use cranpose_core::NodeId;
use cranpose_ui_layout::Alignment;

/// Specification for Box layout behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSpec {
    pub content_alignment: Alignment,
    pub propagate_min_constraints: bool,
}

impl BoxSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content_alignment(mut self, alignment: Alignment) -> Self {
        self.content_alignment = alignment;
        self
    }

    pub fn propagate_min_constraints(mut self, propagate: bool) -> Self {
        self.propagate_min_constraints = propagate;
        self
    }
}

impl Default for BoxSpec {
    fn default() -> Self {
        Self {
            content_alignment: Alignment::TOP_START,
            propagate_min_constraints: false,
        }
    }
}

/// A layout composable that stacks its children on top of each other.
///
/// Use `Box` to:
/// - Overlay elements (e.g., text over an image).
/// - Size a child to match its parent.
/// - Apply a background or border to a single child.
///
/// # Arguments
///
/// * `modifier` - Modifiers to apply to the box layout.
/// * `spec` - Configuration for content alignment.
/// * `content` - The children composables to layout (z-order is first-to-last).
///
/// # Example
///
/// ```rust,ignore
/// Box(
///     Modifier::size(100.0, 100.0).background(Color::Blue),
///     BoxSpec::default().content_alignment(Alignment::Center),
///     || {
///         Text("Centered", Modifier::empty());
///     }
/// );
/// ```
#[composable]
pub fn Box<F>(modifier: Modifier, spec: BoxSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = BoxMeasurePolicy::new(spec.content_alignment, spec.propagate_min_constraints);
    Layout(modifier, policy, content)
}
