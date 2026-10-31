//! A node of the layout tree, a layout node or a subcompose node, read
//! through one type.

use cranpose_core::{MemoryApplier, Node, NodeError, NodeId};
use cranpose_foundation::SemanticsConfiguration;

use crate::{
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{LayoutNode, LayoutState},
};

pub(super) enum LiveLayoutNode<'a> {
    Layout(&'a LayoutNode),
    Subcompose(&'a SubcomposeLayoutNode),
}

impl LiveLayoutNode<'_> {
    pub(super) fn state(&self) -> LayoutState {
        match self {
            Self::Layout(node) => node.layout_state(),
            Self::Subcompose(node) => node.layout_state(),
        }
    }

    pub(super) fn parent(&self) -> Option<NodeId> {
        match self {
            Self::Layout(node) => node.parent(),
            Self::Subcompose(node) => node.parent(),
        }
    }

    pub(super) fn is_window_root(&self) -> bool {
        matches!(self, Self::Layout(node) if node.is_window_root())
    }

    pub(super) fn with_children<R>(&self, read: impl FnOnce(&[NodeId]) -> R) -> R {
        match self {
            Self::Layout(node) => read(&node.children),
            Self::Subcompose(node) => node.with_active_children(read),
        }
    }

    pub(super) fn semantics(&self) -> Option<SemanticsConfiguration> {
        match self {
            Self::Layout(node) => node.semantics_configuration(),
            Self::Subcompose(node) => node.semantics_configuration(),
        }
    }
}

pub(super) fn read_live_layout_node<R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    mut read: impl FnMut(LiveLayoutNode<'_>) -> R,
) -> Result<Option<R>, NodeError> {
    match applier.with_node::<LayoutNode, _>(node_id, |node| read(LiveLayoutNode::Layout(node))) {
        Ok(value) => return Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => {}
        Err(err) => return Err(err),
    }
    match applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        read(LiveLayoutNode::Subcompose(node))
    }) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}
