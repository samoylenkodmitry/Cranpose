// The weight modifier: the node that tells a row or a column its share of
// the room, and its element.

use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState,
};

use super::hash_f32_value;
use crate::modifier::LayoutWeight;

#[derive(Debug)]
pub(crate) struct WeightNode {
    weight: f32,
    fill: bool,
    state: NodeState,
}

impl WeightNode {
    pub(crate) fn new(weight: f32, fill: bool) -> Self {
        Self {
            weight,
            fill,
            state: NodeState::new(),
        }
    }

    pub(crate) fn layout_weight(&self) -> LayoutWeight {
        LayoutWeight {
            weight: self.weight,
            fill: self.fill,
        }
    }
}

impl DelegatableNode for WeightNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for WeightNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Layout);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WeightElement {
    weight: f32,
    fill: bool,
}

impl WeightElement {
    pub(crate) fn new(weight: f32, fill: bool) -> Self {
        Self { weight, fill }
    }
}

impl Hash for WeightElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.weight);
        self.fill.hash(state);
    }
}

impl ModifierNodeElement for WeightElement {
    type Node = WeightNode;
    const FROM_NODE: Option<fn(&WeightNode) -> Self> = Some(|node| Self {
        weight: node.weight,
        fill: node.fill,
    });

    fn create(&self) -> Self::Node {
        WeightNode::new(self.weight, self.fill)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.weight != self.weight || node.fill != self.fill {
            node.weight = self.weight;
            node.fill = self.fill;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
