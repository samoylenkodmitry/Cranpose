//! ScrollNode - Layout modifier for scrollable content.
//!
//! This module provides ScrollNode, a LayoutModifierNode that measures content
//! with infinite constraints on the scroll axis and offsets placement based on
//! scroll position. Matches Jetpack Compose's ScrollNode (Scroll.kt:431-520).

use compose_foundation::scroll::ScrollState;
use compose_foundation::{
    Constraints, DelegatableNode, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};
use compose_ui_layout::LayoutModifierMeasureResult;
use std::hash::{Hash, Hasher};

/// Layout modifier node that handles scrolling by measuring content with 
/// infinite constraints and offsetting placement.
#[derive(Debug)]
pub struct ScrollNode {
    state: ScrollState,
    reverse_scrolling: bool,
    is_vertical: bool,
    node_state: NodeState,
}

impl ScrollNode {
    pub fn new(state: ScrollState, is_vertical: bool, reverse_scrolling: bool) -> Self {
        Self {
            state,
            reverse_scrolling,
            is_vertical,
            node_state: NodeState::new(),
        }
    }
}

impl DelegatableNode for ScrollNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for ScrollNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for ScrollNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let scroll_range = self.state.max_value();
        let scroll_value = self.state.value();
        // Measure child with infinite constraints on the scroll axis
        let child_constraints = if self.is_vertical {
            Constraints {
                min_width: constraints.min_width,
                max_width: constraints.max_width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            }
        } else {
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: constraints.min_height,
                max_height: constraints.max_height,
            }
        };

        let placeable = measurable.measure(child_constraints);
        
        // Constrain to parent's max constraints
        let width = placeable.width().min(constraints.max_width);
        let height = placeable.height().min(constraints.max_height);

        // Calculate scroll range (how much content exceeds viewport)
        let scroll_range = if self.is_vertical {
            ((placeable.height() - height) as i32).max(0)
        } else {
            ((placeable.width() - width) as i32).max(0)
        };

        // Update scroll state with bounds
        self.state.set_max_value(scroll_range);
        self.state.set_viewport_size(if self.is_vertical { height as i32 } else { width as i32 });

        // Calculate offset based on scroll position
        let scroll = self.state.value().clamp(0, scroll_range);
        let abs_scroll = if self.reverse_scrolling {
            scroll - scroll_range
        } else {
            -scroll
        } as f32;

        let x_offset = if self.is_vertical { 0.0 } else { abs_scroll };
        let y_offset = if self.is_vertical { abs_scroll } else { 0.0 };

        // Return measurement with offset
        LayoutModifierMeasureResult::new(
            Size { width, height },
            x_offset,
            y_offset,
        )
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.min_intrinsic_width(if self.is_vertical { f32::INFINITY } else { height })
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        measurable.max_intrinsic_width(if self.is_vertical { f32::INFINITY } else { height })
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.min_intrinsic_height(if self.is_vertical { width } else { f32::INFINITY })
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        measurable.max_intrinsic_height(if self.is_vertical { width } else { f32::INFINITY })
    }
}

/// Element that creates and updates ScrollNode instances.
#[derive(Debug, Clone)]
pub struct ScrollNodeElement {
    state: ScrollState,
    is_vertical: bool,
    reverse_scrolling: bool,
}

impl ScrollNodeElement {
    pub fn new(state: ScrollState, is_vertical: bool, reverse_scrolling: bool) -> Self {
        Self {
            state,
            is_vertical,
            reverse_scrolling,
        }
    }
}

impl PartialEq for ScrollNodeElement {
    fn eq(&self, other: &Self) -> bool {
        // States are equal if they point to the same data
        std::ptr::eq(
            &*self.state.data.borrow() as *const _,
            &*other.state.data.borrow() as *const _,
        ) && self.is_vertical == other.is_vertical
            && self.reverse_scrolling == other.reverse_scrolling
    }
}

impl Hash for ScrollNodeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the data pointer address
        (&*self.state.data.borrow() as *const _ as usize).hash(state);
        self.is_vertical.hash(state);
        self.reverse_scrolling.hash(state);
    }
}

impl ModifierNodeElement for ScrollNodeElement {
    type Node = ScrollNode;

    fn create(&self) -> Self::Node {
        ScrollNode::new(self.state.clone(), self.is_vertical, self.reverse_scrolling)
    }

    fn update(&self, node: &mut Self::Node) {
        node.state = self.state.clone();
        node.is_vertical = self.is_vertical;
        node.reverse_scrolling = self.reverse_scrolling;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
