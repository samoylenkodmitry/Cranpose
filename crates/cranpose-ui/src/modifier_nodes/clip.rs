// The clip-to-bounds modifier: its node and element.

use std::hash::Hash;

use cranpose_foundation::{
    DelegatableNode, DrawModifierNode, InvalidationKind, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState,
};

use super::impl_draw_modifier_node;

#[derive(Debug)]
pub(crate) struct ClipToBoundsNode {
    state: NodeState,
}

impl ClipToBoundsNode {
    pub(crate) fn new() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for ClipToBoundsNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl_draw_modifier_node!(ClipToBoundsNode);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ClipToBoundsElement;

impl ClipToBoundsElement {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl ModifierNodeElement for ClipToBoundsElement {
    type Node = ClipToBoundsNode;
    const FROM_NODE: Option<fn(&ClipToBoundsNode) -> Self> = Some(|_| Self);

    fn create(&self) -> Self::Node {
        ClipToBoundsNode::new()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}
