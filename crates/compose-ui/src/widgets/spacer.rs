//! Spacer widget implementation

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::LeafMeasurePolicy;
use crate::modifier::{Modifier, Size};
use crate::widgets::Layout;
use compose_core::NodeId;

/// Creates a spacer with the specified size.
///
/// This is now implemented using LayoutNode with LeafMeasurePolicy,
/// following the Jetpack Compose pattern of using Layout for all widgets.
#[composable]
pub fn Spacer(size: Size) -> NodeId {
    Layout(
        Modifier::empty(),
        LeafMeasurePolicy::new(size),
        || {}, // No children
    )
}
