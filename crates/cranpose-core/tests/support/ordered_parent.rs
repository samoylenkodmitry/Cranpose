use cranpose_core::{Node, NodeId};

pub trait FixtureLeaf: std::any::Any {
    fn update(&mut self) {}
}

pub struct ParentTracked<T> {
    pub node: T,
    parent: Option<NodeId>,
}

impl<T> ParentTracked<T> {
    pub fn new(node: T) -> Self {
        Self { node, parent: None }
    }
}

impl<T: FixtureLeaf> Node for ParentTracked<T> {
    fn update(&mut self) {
        FixtureLeaf::update(&mut self.node);
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        if let Some(previous) = self.parent.replace(parent) {
            debug_assert_eq!(
                previous, parent,
                "a parent link must be cleared before reparenting a fixture node"
            );
        }
    }

    fn on_removed_from_parent(&mut self) {
        let previous = self.parent.take();
        debug_assert!(
            previous.is_some(),
            "a fixture node must be attached before it is detached"
        );
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

#[derive(Default)]
pub struct OrderedParent {
    pub children: Vec<NodeId>,
    pub hidden_children: Vec<NodeId>,
}

impl Node for OrderedParent {
    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let previous_len = self.children.len();
        self.children.retain(|&id| id != child);
        self.children.len() != previous_len
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        self.children.insert(to.min(self.children.len()), child);
    }

    fn collect_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend(
            self.children
                .iter()
                .filter(|child| !self.hidden_children.contains(child))
                .copied(),
        );
    }

    fn collect_owned_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend_from_slice(&self.children);
    }

    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        self.children.iter().position(|&id| id == child)
    }
}
