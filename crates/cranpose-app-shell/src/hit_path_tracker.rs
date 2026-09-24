use std::collections::HashMap;

use cranpose_core::NodeId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PointerId(pub u32);

impl PointerId {
    pub const PRIMARY: PointerId = PointerId(0);
}

pub struct HitPathTracker {
    paths: HashMap<PointerId, Vec<Vec<NodeId>>>,
}

impl HitPathTracker {
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
        }
    }

    pub fn add_hit_path(&mut self, pointer: PointerId, capture_paths: Vec<Vec<NodeId>>) {
        if capture_paths.is_empty() {
            self.paths.remove(&pointer);
        } else {
            self.paths.insert(pointer, capture_paths);
        }
    }

    pub fn get_path(&self, pointer: PointerId) -> Option<&[Vec<NodeId>]> {
        self.paths.get(&pointer).map(Vec::as_slice)
    }

    pub fn dispatch_order(&self, pointer: PointerId) -> Option<Vec<NodeId>> {
        self.get_path(pointer).map(dispatch_order_for_paths)
    }

    pub fn remove_path(&mut self, pointer: PointerId) -> Option<Vec<Vec<NodeId>>> {
        self.paths.remove(&pointer)
    }

    pub fn has_path(&self, pointer: PointerId) -> bool {
        self.paths.contains_key(&pointer)
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

impl Default for HitPathTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct DispatchNode {
    children: Vec<NodeId>,
}

pub(crate) fn dispatch_order_for_paths(paths: &[Vec<NodeId>]) -> Vec<NodeId> {
    fn push_unique(nodes: &mut Vec<NodeId>, node_id: NodeId) {
        if !nodes.contains(&node_id) {
            nodes.push(node_id);
        }
    }

    fn visit(node_id: NodeId, tree: &HashMap<NodeId, DispatchNode>, ordered: &mut Vec<NodeId>) {
        if let Some(node) = tree.get(&node_id) {
            for &child in &node.children {
                visit(child, tree, ordered);
            }
        }
        ordered.push(node_id);
    }

    let mut roots = Vec::new();
    let mut tree: HashMap<NodeId, DispatchNode> = HashMap::new();

    for path in paths {
        let mut parent = None;
        for node_id in path.iter().rev().copied() {
            tree.entry(node_id).or_default();
            if let Some(parent_id) = parent {
                let parent_node = tree.entry(parent_id).or_default();
                push_unique(&mut parent_node.children, node_id);
            } else {
                push_unique(&mut roots, node_id);
            }
            parent = Some(node_id);
        }
    }

    let mut ordered = Vec::new();
    for root in roots {
        visit(root, &tree, &mut ordered);
    }
    ordered
}

#[cfg(test)]
#[path = "tests/hit_path_tracker_tests.rs"]
mod tests;
