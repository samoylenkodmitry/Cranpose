use cranpose_core::{
    NodeId,
    collections::{growth_slack::GrowthSlack, map::HashSet},
};

pub(super) struct DirtyNodeSet {
    pub(super) dirty_nodes: HashSet<NodeId>,
}

impl DirtyNodeSet {
    pub(super) fn new() -> Self {
        Self {
            dirty_nodes: HashSet::default(),
        }
    }

    pub(super) fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    pub(super) fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    // A drain clears every control byte of the table when it ends, even of
    // an empty one: most frames take sets that are empty.
    pub(super) fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        if self.dirty_nodes.is_empty() {
            return Vec::new();
        }
        self.dirty_nodes.drain().collect()
    }

    pub(super) fn take_dirty_nodes_into(&mut self, output: &mut Vec<NodeId>) {
        output.clear();
        if self.dirty_nodes.is_empty() {
            return;
        }
        output.extend(self.dirty_nodes.drain());
    }

    // Gives back the room a burst of nodes took.
    pub(super) fn trim(&mut self) {
        self.dirty_nodes.trim_growth_slack();
    }

    // Appends the nodes, sorted, to `nodes`; they stay in the set.
    pub(super) fn append_sorted(&self, nodes: &mut Vec<NodeId>) {
        let start = nodes.len();
        nodes.extend(self.dirty_nodes.iter().copied());
        nodes[start..].sort_unstable();
    }
}
