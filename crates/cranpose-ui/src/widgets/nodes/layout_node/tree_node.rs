use std::any::TypeId;

use cranpose_core::{Node, NodeId};

use super::{LayoutEnvironment, LayoutNode, ModifierChainHandle, unregister_layout_node};
use crate::widgets::nodes::node_pool_limit::{KEPT_CHILD_ROOM, node_pool_limit};

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

    fn mount_under(&mut self, parent: NodeId, id: NodeId) {
        self.mount_under_made_parent(parent, id);
        Node::mount(self);
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
        node_pool_limit()
    }

    // The node leaves the tree whole: it lets go of what a drop would let go
    // of and keeps the room of its parts and its lists for the node that
    // takes its place.
    fn prepare_for_recycle(&mut self) {
        let Self {
            parts,
            children,
            density: _,
            is_virtual: _,
            needs_semantics,
            descendant_needs_semantics,
            needs_redraw,
            needs_pointer_pass,
            needs_focus_sync,
            modifier_slices_dirty,
            debug_modifiers,
            id,
            parent,
            folded_parent,
            modifier_slices_snapshot,
            semantics_reach,
            owner_context_id,
            modifier_chain,
            #[cfg(feature = "inspection")]
                source_trace: _,
        } = self;
        if let (Some(id), Some(owner_context_id)) = (id.replace(None), owner_context_id.take()) {
            unregister_layout_node(owner_context_id, id);
        }
        parts.release_for_recycle();
        if children.capacity() > KEPT_CHILD_ROOM {
            *children = Vec::new();
        } else {
            children.clear();
        }
        *modifier_chain = ModifierChainHandle::detached(LayoutEnvironment::default().direction);
        *modifier_slices_snapshot = crate::modifier::SlicesSnapshot::default();
        parent.set(None);
        folded_parent.set(None);
        semantics_reach.set(None);
        needs_semantics.set(false);
        descendant_needs_semantics.set(false);
        needs_redraw.set(false);
        needs_pointer_pass.set(false);
        needs_focus_sync.set(false);
        debug_modifiers.set(false);
        modifier_slices_dirty.set(true);
    }

    fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
        // The box comes first, so the shell is made in it.
        Some(Box::write(
            Box::new_uninit(),
            Self::new_recycled_shell(self.is_virtual),
        ))
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
