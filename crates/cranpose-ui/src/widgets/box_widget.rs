//! Box widget implementation

use cranpose_core::NodeId;
use cranpose_ui_layout::Alignment;

use super::layout::compose_layout;
use crate::{composable, layout::policies::BoxMeasurePolicy, modifier::Modifier};

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

/// Places children over the same area. `content_alignment` selects each child's
/// position within the box; later children draw above earlier children.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Badge() {
///     Box(
///         Modifier::empty()
///             .size_points(100.0, 100.0)
///             .background(Color(0.1, 0.3, 0.9, 1.0)),
///         BoxSpec::default().content_alignment(Alignment::CENTER),
///         || {
///             Text("Centered", Modifier::empty(), TextStyle::default());
///         },
///     );
/// }
/// ```
#[composable]
pub fn Box<F>(modifier: Modifier, spec: BoxSpec, content: F) -> NodeId
where
    F: FnMut() + 'static,
{
    let policy = BoxMeasurePolicy::new(spec.content_alignment, spec.propagate_min_constraints);
    compose_layout(modifier, policy, crate::density::density(), content)
}
