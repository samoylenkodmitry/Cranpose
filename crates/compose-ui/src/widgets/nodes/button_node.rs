use crate::{layout::mark_measure_dirty, modifier::Modifier};
use compose_core::{Node, NodeId};
use indexmap::IndexSet;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub struct ButtonNode {
    pub modifier: Modifier,
    pub on_click: Rc<RefCell<dyn FnMut()>>,
    pub children: IndexSet<NodeId>,
    id: Option<NodeId>,
}

impl Default for ButtonNode {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            on_click: Rc::new(RefCell::new(|| {})),
            children: IndexSet::new(),
            id: None,
        }
    }
}

impl ButtonNode {
    pub fn trigger(&self) {
        (self.on_click.borrow_mut())();
    }

    pub fn set_node_id(&mut self, id: NodeId) {
        self.id = Some(id);
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        if self.modifier == modifier {
            return;
        }
        self.modifier = modifier;
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }
}

impl Node for ButtonNode {
    fn insert_child(&mut self, child: NodeId) {
        self.children.insert(child);
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn remove_child(&mut self, child: NodeId) {
        self.children.shift_remove(&child);
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let mut ordered: Vec<NodeId> = self.children.iter().copied().collect();
        let child = ordered.remove(from);
        let target = to.min(ordered.len());
        ordered.insert(target, child);
        self.children.clear();
        for id in ordered {
            self.children.insert(id);
        }
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        for &child in children {
            self.children.insert(child);
        }
        if let Some(id) = self.id {
            mark_measure_dirty(id);
        }
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.iter().copied().collect()
    }
}
