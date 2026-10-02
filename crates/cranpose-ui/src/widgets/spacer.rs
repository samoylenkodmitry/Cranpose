//! Spacer widget implementation

use cranpose_core::NodeId;

use crate::{
    composable, layout::policies::EmptyMeasurePolicy, modifier::Modifier,
    widgets::layout::compose_leaf_layout,
};

/// A component that represents an empty space.
///
/// # When to use
/// Use `Spacer` to create empty space between other composables, or to push
/// composables apart when using weighted arrangements in `Row` or `Column`.
///
/// # Arguments
///
/// * `modifier` - What sizes the space: it has no size of its own, as
///   Compose's `Spacer(modifier)` has none.
///
/// # Example
///
/// ```rust,ignore
/// Row(..., || {
///     Text("Left", Modifier::empty());
///     Spacer(Modifier::empty().width(16.0)); // 16dp gap
///     Text("Right", Modifier::empty());
/// });
/// ```
#[composable]
pub fn Spacer(modifier: Modifier) -> NodeId {
    compose_leaf_layout(modifier, EmptyMeasurePolicy, crate::density::density())
}
