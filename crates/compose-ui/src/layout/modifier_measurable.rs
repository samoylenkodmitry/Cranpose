//! Measurable wrappers for chaining LayoutModifierNode::measure() calls.
//!
//! This module provides a simplified implementation of Jetpack Compose's NodeCoordinator
//! pattern, focused specifically on the measurement pipeline. Instead of full coordinators,
//! we use measurable wrappers that chain modifier node measurements.

use compose_core::NodeId;
use compose_foundation::{BasicModifierNodeContext, LayoutModifierNode};
use compose_ui_graphics::Size;
use compose_ui_layout::{Constraints, Measurable, MeasurePolicy, MeasureResult, Placeable};
use std::cell::RefCell;
use std::rc::Rc;

/// A measurable that wraps a LayoutModifierNode and delegates to the wrapped measurable.
/// This enables chaining of modifier node measurements, matching Jetpack Compose's
/// LayoutModifierNodeCoordinator behavior.
pub(crate) struct ModifierNodeMeasurable {
    node: Rc<RefCell<dyn LayoutModifierNode>>,
    wrapped: Box<dyn Measurable>,
}

impl ModifierNodeMeasurable {
    pub fn new(
        node: Rc<RefCell<dyn LayoutModifierNode>>,
        wrapped: Box<dyn Measurable>,
    ) -> Self {
        Self { node, wrapped }
    }
}

impl Measurable for ModifierNodeMeasurable {
    fn measure(&self, constraints: Constraints) -> Box<dyn Placeable> {
        // Create a context for the modifier node
        let mut context = BasicModifierNodeContext::new();

        // Invoke the layout modifier node's measure method, passing the wrapped measurable
        // This matches Jetpack Compose's pattern:
        // with(layoutModifierNode) { measure(wrappedNonNull, constraints) }
        let size = self
            .node
            .borrow_mut()
            .measure(&mut context, &*self.wrapped, constraints);

        // Return a simple placeable with the measured size
        Box::new(SimplePlaceable { size })
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        // TODO: Properly delegate to layoutModifierNode.minIntrinsicWidth
        // For now, delegate to wrapped
        self.wrapped.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        // TODO: Properly delegate to layoutModifierNode.maxIntrinsicWidth
        // For now, delegate to wrapped
        self.wrapped.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        // TODO: Properly delegate to layoutModifierNode.minIntrinsicHeight
        // For now, delegate to wrapped
        self.wrapped.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        // TODO: Properly delegate to layoutModifierNode.maxIntrinsicHeight
        // For now, delegate to wrapped
        self.wrapped.max_intrinsic_height(width)
    }
}

/// Simple placeable implementation that just holds a size.
/// In a full implementation, this would handle placement coordinates,
/// but for measurement-only we just need the size.
struct SimplePlaceable {
    size: Size,
}

impl Placeable for SimplePlaceable {
    fn width(&self) -> f32 {
        self.size.width
    }

    fn height(&self) -> f32 {
        self.size.height
    }

    fn place(&self, _x: f32, _y: f32) {
        // Placement is handled elsewhere in the layout system
        // This simplified implementation doesn't track placement
    }

    fn node_id(&self) -> NodeId {
        // This simplified placeable doesn't track the actual node ID
        // A full coordinator implementation would maintain this
        NodeId::default()
    }
}
