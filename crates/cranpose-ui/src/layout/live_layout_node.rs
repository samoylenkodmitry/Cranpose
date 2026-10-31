//! A node of the layout tree, a layout node or a subcompose node, read
//! through one type.

use cranpose_core::{Applier, MemoryApplier, Node, NodeError, NodeId};
use cranpose_foundation::SemanticsConfiguration;

use crate::{
    render_state::AppContext,
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{LayoutNode, LayoutParts, LayoutState, NodeCache},
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

    // A slot host of a subcompose node, which lays out nothing itself: its
    // subcompose node measures and places the slot's content.
    pub(super) fn is_virtual(&self) -> bool {
        matches!(self, Self::Layout(node) if Node::is_virtual(*node))
    }

    fn parts(&self) -> &LayoutParts {
        match self {
            Self::Layout(node) => node.layout_parts(),
            Self::Subcompose(node) => node.layout_parts(),
        }
    }

    // The node's measurement cache, which holds its layout flags.
    pub(super) fn cache(&self) -> &NodeCache {
        self.parts().cache()
    }

    pub(super) fn is_placed(&self) -> bool {
        self.parts().layout_state().borrow().is_placed()
    }

    pub(super) fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        match self {
            Self::Layout(node) => node.parent_data(),
            Self::Subcompose(node) => node.parent_data(),
        }
    }

    // Places the node again where `placer` last placed it.
    pub(super) fn stay_placed(&self, placer: NodeId, context: &AppContext) {
        let mut state = self.parts().layout_state().borrow_mut();
        let position = state.position();
        state.place(position, Some(placer), context);
    }
}

pub(super) fn read_live_layout_node<R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(LiveLayoutNode<'_>) -> R,
) -> Result<Option<R>, NodeError> {
    let node = match applier.get_mut(node_id) {
        Ok(node) => node.as_any_mut(),
        Err(NodeError::Missing { .. }) => return Ok(None),
        Err(err) => return Err(err),
    };
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return Ok(Some(read(LiveLayoutNode::Layout(node))));
    }
    Ok(node
        .downcast_ref::<SubcomposeLayoutNode>()
        .map(|node| read(LiveLayoutNode::Subcompose(node))))
}
