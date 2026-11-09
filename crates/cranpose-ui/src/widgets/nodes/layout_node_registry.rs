//! The registry of an app's layout nodes: each node's parent and modifier
//! locals, through which a modifier chain resolves the locals its ancestors
//! provide.

use std::cell::RefCell;

use cranpose_core::{
    NodeId, PackedNodeId,
    collections::{id_pages::IdPages, map::HashMap},
};
use cranpose_foundation::NodeCapabilities;

use super::LayoutNode;
use crate::modifier::{
    ModifierLocalSource, ModifierLocalToken, ModifierLocalsHandle, ResolvedModifierLocal,
};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct LayoutNodeRegistryDebugStats {
    pub(super) len: usize,
    pub(super) capacity: usize,
}

// The parent of each registered node by node id: ids come from a counter,
// so the pages hold the nodes of a screen side by side. The modifier locals
// sit apart, for the few chains that hold any.
pub(crate) struct LayoutNodeRegistryState {
    parents: RefCell<IdPages<Option<PackedNodeId>>>,
    locals: RefCell<HashMap<NodeId, ModifierLocalsHandle>>,
}

impl LayoutNodeRegistryState {
    pub(crate) fn new() -> Self {
        Self {
            parents: RefCell::default(),
            locals: RefCell::default(),
        }
    }

    fn register(&self, id: NodeId, node: &LayoutNode) {
        self.parents
            .borrow_mut()
            .update(id.0, |slot| *slot = Some(PackedNodeId::from(node.parent())));
        self.keep_locals(
            id,
            node.modifier_child_capabilities(),
            node.modifier_locals_handle(),
        );
    }

    fn unregister(&self, id: NodeId) {
        self.parents.borrow_mut().update(id.0, |slot| *slot = None);
        self.locals.borrow_mut().remove(&id);
    }

    // Keeps the modifier locals of node `id` while its chain holds some.
    fn keep_locals(
        &self,
        id: NodeId,
        capabilities: NodeCapabilities,
        modifier_locals: Option<ModifierLocalsHandle>,
    ) {
        let held =
            modifier_locals.filter(|_| capabilities.contains(NodeCapabilities::MODIFIER_LOCALS));
        match held {
            Some(locals) => {
                self.locals.borrow_mut().insert(id, locals);
            }
            None => {
                let mut kept = self.locals.borrow_mut();
                if !kept.is_empty() {
                    kept.remove(&id);
                }
            }
        }
    }

    pub(super) fn update_entry(
        &self,
        id: NodeId,
        parent: Option<NodeId>,
        modifier_child_capabilities: NodeCapabilities,
        modifier_locals: Option<ModifierLocalsHandle>,
    ) {
        let registered = match self
            .parents
            .borrow_mut()
            .entry_mut(id.0)
            .and_then(Option::as_mut)
        {
            Some(slot) => {
                *slot = PackedNodeId::from(parent);
                true
            }
            None => false,
        };
        if registered {
            self.keep_locals(id, modifier_child_capabilities, modifier_locals);
        }
    }

    #[cfg(test)]
    fn stats(&self) -> LayoutNodeRegistryDebugStats {
        let parents = self.parents.borrow();
        LayoutNodeRegistryDebugStats {
            len: parents.occupied(),
            capacity: parents.capacity(),
        }
    }

    fn resolve_modifier_local_from_parent_chain(
        &self,
        start: Option<NodeId>,
        token: &ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        if self.locals.borrow().is_empty() {
            return None;
        }
        let mut current = start;
        while let Some(node) = current {
            let locals = self.locals.borrow().get(&node).cloned();
            let resolved = locals
                .and_then(|locals| locals.borrow().resolve(token))
                .map(|value| value.with_source(ModifierLocalSource::Ancestor));
            if resolved.is_some() {
                return resolved;
            }
            current = self
                .parents
                .borrow()
                .entry(node.0)
                .copied()
                .flatten()
                .and_then(PackedNodeId::get);
        }
        None
    }
}

pub(super) fn register_layout_node(
    id: NodeId,
    node: &LayoutNode,
) -> crate::render_state::AppContextId {
    crate::render_state::with_current_layout_node_registry(|registry| registry.register(id, node))
}

pub(super) fn unregister_layout_node(
    owner_context_id: crate::render_state::AppContextId,
    id: NodeId,
) {
    let _ = crate::render_state::with_layout_node_registry_by_app_context(
        owner_context_id,
        |registry| registry.unregister(id),
    );
    crate::render_state::measure_reads::forget(owner_context_id, id);
}

#[cfg(test)]
pub(super) fn layout_node_registry_stats() -> LayoutNodeRegistryDebugStats {
    crate::render_state::with_layout_node_registry(LayoutNodeRegistryState::stats)
}

pub(super) fn resolve_modifier_local_from_parent_chain(
    start: Option<NodeId>,
    token: &ModifierLocalToken,
) -> Option<ResolvedModifierLocal> {
    crate::render_state::with_layout_node_registry(|registry| {
        registry.resolve_modifier_local_from_parent_chain(start, token)
    })
}
