use cranpose_core::{NodeId, collections::node_bits::NodeBits};

use super::{geometry_scene_nodes::MovedSceneNode, taken_list::TakenList};

/// The nodes a layout pass names to the scene, each once a take: the list
/// keeps a bit for each node it named since its last take, so a second note
/// in the same take is dropped with one bit test instead of a hash.
pub(super) struct NotedSceneNodes<T> {
    nodes: TakenList<T>,
    noted: NodeBits,
}

// What a list of noted nodes holds for each node: the node it names.
pub(super) trait NotedNode {
    fn node(&self) -> NodeId;
}

impl NotedNode for NodeId {
    fn node(&self) -> NodeId {
        *self
    }
}

impl NotedNode for MovedSceneNode {
    fn node(&self) -> NodeId {
        self.node
    }
}

impl<T: NotedNode> NotedSceneNodes<T> {
    pub(super) fn new() -> Self {
        Self {
            nodes: TakenList::default(),
            noted: NodeBits::default(),
        }
    }

    pub(super) fn note(&mut self, node: T) {
        if self.noted.insert(node.node()) {
            self.nodes.push(node);
        }
    }

    // Moves the nodes into `output`, dropping what `output` held. A node
    // the list held may be noted again.
    pub(super) fn take_into(&mut self, output: &mut Vec<T>) {
        self.nodes.take_into(output);
        for node in output.iter() {
            self.noted.remove(node.node());
        }
    }
}
