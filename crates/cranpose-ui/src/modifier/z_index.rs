use std::hash::{Hash, Hasher};

use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState,
};

use super::{Modifier, inspector_metadata};

impl Modifier {
    /// Sets where this node draws among its siblings: a sibling with a larger
    /// `z_index` draws on top and takes a tap first; siblings with the same
    /// one keep the order the parent placed them in. Several `z_index`
    /// modifiers on one node add up. The order a screen reader reads in does
    /// not change.
    ///
    /// This is Compose's `Modifier.zIndex(zIndex)`.
    ///
    /// Example: `Modifier::empty().z_index(1.0)`
    pub fn z_index(self, z_index: f32) -> Self {
        let modifier = Self::with_element(ZIndexElement { z_index }).with_inspector_metadata(
            inspector_metadata("zIndex", move |info| {
                info.add_property("zIndex", z_index.to_string());
            }),
        );
        self.then(modifier)
    }
}

/// Whether a `z_index` modifier is attached anywhere on this thread. While
/// none is, the scene keeps every node's children in composition order
/// without reading their z indices.
pub fn z_index_in_use() -> bool {
    crate::thread_state::cells(|cells| cells.z_index_nodes.get()) > 0
}

fn count_z_index_node(attached: bool) {
    crate::thread_state::cells(|cells| {
        let nodes = &cells.z_index_nodes;
        nodes.set(if attached {
            nodes.get() + 1
        } else {
            nodes.get().saturating_sub(1)
        });
    });
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ZIndexElement {
    z_index: f32,
}

impl Hash for ZIndexElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u32(self.z_index.to_bits());
    }
}

impl ModifierNodeElement for ZIndexElement {
    type Node = ZIndexNode;

    fn create(&self) -> Self::Node {
        ZIndexNode {
            z_index: self.z_index,
            owner: None,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        if node.z_index.to_bits() != self.z_index.to_bits() {
            node.z_index = self.z_index;
            node.reorder();
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

/// The node behind [`Modifier::z_index`]. It draws nothing; the modifier
/// slices add its value up into the node's z index.
pub(crate) struct ZIndexNode {
    z_index: f32,
    /// The layout node whose chain holds this node.
    owner: Option<cranpose_core::NodeId>,
    state: NodeState,
}

impl ZIndexNode {
    pub(crate) fn z_index(&self) -> f32 {
        self.z_index
    }

    /// The order lives in the owner's parent's layer, which a change of the
    /// owner alone does not rebuild.
    fn reorder(&self) {
        if let Some(owner) = self.owner {
            crate::schedule_z_order_change(owner);
        }
    }
}

impl DelegatableNode for ZIndexNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ZIndexNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::Draw);
        count_z_index_node(true);
        self.owner = context.node_id();
        self.reorder();
    }

    fn on_detach(&mut self) {
        count_z_index_node(false);
        self.reorder();
    }
}
