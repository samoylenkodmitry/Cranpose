// The background modifier and the corner shape it draws in: their nodes and
// elements.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    DelegatableNode, DrawModifierNode, InvalidationKind, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState,
};

use super::{hash_f32_value, impl_draw_modifier_node};
use crate::modifier::{Color, RoundedCornerShape};

/// Node that draws a background behind its content.
#[derive(Debug)]
pub struct BackgroundNode {
    color: Color,
    shape: Option<RoundedCornerShape>,
    state: NodeState,
}

impl BackgroundNode {
    /// Creates a node that fills its bounds with `color` behind its content, with square corners.
    pub fn new(color: Color) -> Self {
        Self {
            color,
            shape: None,
            state: NodeState::new(),
        }
    }

    /// Returns the fill color.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Returns the corner shape of the fill, or `None` for square corners.
    pub fn shape(&self) -> Option<RoundedCornerShape> {
        self.shape
    }
}

impl DelegatableNode for BackgroundNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_draw_modifier_node!(BackgroundNode);

/// Element that creates and updates background nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    /// Creates an element for a node that fills its bounds with `color`.
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl Hash for BackgroundElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.color.0);
        hash_f32_value(state, self.color.1);
        hash_f32_value(state, self.color.2);
        hash_f32_value(state, self.color.3);
    }
}

impl ModifierNodeElement for BackgroundElement {
    type Node = BackgroundNode;

    fn create(&self) -> Self::Node {
        BackgroundNode::new(self.color)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.color != self.color {
            node.color = self.color;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

/// Node that tracks the latest rounded corner shape.
#[derive(Debug)]
pub struct CornerShapeNode {
    shape: RoundedCornerShape,
    state: NodeState,
}

impl CornerShapeNode {
    /// Creates a node that holds `shape` as the current rounded corner shape.
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self {
            shape,
            state: NodeState::new(),
        }
    }

    /// Returns the corner shape.
    pub fn shape(&self) -> RoundedCornerShape {
        self.shape
    }
}

impl DelegatableNode for CornerShapeNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_draw_modifier_node!(CornerShapeNode);

/// Element that creates and updates corner shape nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerShapeElement {
    shape: RoundedCornerShape,
}

impl CornerShapeElement {
    /// Creates an element for a node that holds `shape` as the current rounded corner shape.
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self { shape }
    }
}

impl Hash for CornerShapeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let radii = self.shape.radii();
        hash_f32_value(state, radii.top_left);
        hash_f32_value(state, radii.top_right);
        hash_f32_value(state, radii.bottom_right);
        hash_f32_value(state, radii.bottom_left);
    }
}

impl ModifierNodeElement for CornerShapeElement {
    type Node = CornerShapeNode;

    fn create(&self) -> Self::Node {
        CornerShapeNode::new(self.shape)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.shape != self.shape {
            node.shape = self.shape;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}
