use std::any::TypeId;

use cranpose_core::{Node, NodeId};

use super::LayoutNode;

impl Node for LayoutNode {
    fn mount(&mut self) {
        self.modifier_chain.remount(self.id.get());
        crate::modal_nodes::reach_changed(self.id.get());
    }

    fn unmount(&mut self) {
        self.modifier_chain.chain_mut().detach_nodes();
    }

    fn set_node_id(&mut self, id: NodeId) {
        Self::set_node_id(self, id);
    }

    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        self.parts.cache().clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        // A child is listed once, and a child sync removes children from
        // the last: the search from the end finds it at once.
        let at = self.children.iter().rposition(|&id| id == child);
        let removed = at.map(|at| self.children.remove(at)).is_some();
        if removed {
            self.parts.cache().clear();
            self.mark_needs_measure();
            self.note_semantics_layout_change();
        }
        removed
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        let target = to.min(self.children.len());
        self.children.insert(target, child);
        self.parts.cache().clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        self.children.extend_from_slice(children);
        self.parts.cache().clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
    }

    fn collect_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend_from_slice(&self.children);
    }

    fn for_each_child(&self, f: &mut dyn FnMut(NodeId)) {
        for &child in &self.children {
            f(child);
        }
    }

    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        self.children.iter().position(|&id| id == child)
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.set_parent(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.clear_parent();
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    fn mark_needs_layout(&self) {
        Self::mark_needs_layout(self);
    }

    fn needs_layout(&self) -> bool {
        Self::needs_layout(self)
    }

    fn mark_needs_measure(&self) {
        Self::mark_needs_measure(self);
    }

    fn needs_measure(&self) -> bool {
        Self::needs_measure(self)
    }

    fn mark_descendant_needs_layout(&self, measure: bool) {
        self.parts.cache().mark_descendant_needs_layout(measure);
    }

    fn descendant_needs_layout(&self) -> bool {
        self.parts.cache().dirt().descendant.layout
    }

    fn is_virtual(&self) -> bool {
        self.is_virtual
    }

    fn descendant_needs_measure(&self) -> bool {
        self.parts.cache().dirt().descendant.measure
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
        self.forget_semantics_reach();
    }

    fn mark_descendant_needs_semantics(&self) {
        self.descendant_needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        Self::needs_semantics(self)
    }

    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        if self.parent.get().is_none() {
            self.parent.set(Some(parent));
        }
    }

    fn recycle_key(&self) -> Option<TypeId> {
        Some(TypeId::of::<Self>())
    }

    fn recycle_pool_limit(&self) -> Option<usize> {
        Some(super::super::node_pool_limit::node_pool_limit())
    }

    fn prepare_for_recycle(&mut self) {
        *self = Self::new_recycled_shell(self.is_virtual);
    }

    fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
        Some(Box::new(Self::new_recycled_shell(self.is_virtual)))
    }

    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn Node>> {
        // The whole node moves into the new box. Its modifier nodes keep
        // what the user is doing with it, such as a focus, a held press or an
        // IME composition, and its caches and registry entry stay true for
        // it. Only its child list, which a spike of children may have grown,
        // is cut to size. The shell left behind has no id to unregister.
        let mut node = std::mem::replace(self, Self::new_recycled_shell(self.is_virtual));
        node.children.shrink_to_fit();
        Some(Box::new(node))
    }
}
