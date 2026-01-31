//! Spacer widget implementation

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::LeafMeasurePolicy;
use crate::modifier::{Modifier, Size};
use crate::widgets::Layout;
use cranpose_core::NodeId;

/// A component that represents an empty space.
///
/// # When to use
/// Use `Spacer` to create empty space between other composables, or to push
/// composables apart when using weighted arrangements in `Row` or `Column`.
///
/// # Arguments
///
/// * `size` - The explicit size of the spacer.
///
/// # Example
///
/// ```rust,ignore
/// Row(..., || {
///     Text("Left", Modifier::empty());
///     Spacer(Size::new(16.0, 0.0)); // 16dp gap
///     Text("Right", Modifier::empty());
/// });
/// ```
#[composable]
pub fn Spacer(size: Size) -> NodeId {
    Layout(
        Modifier::empty(),
        LeafMeasurePolicy::new(size),
        || {}, // No children
    )
}
