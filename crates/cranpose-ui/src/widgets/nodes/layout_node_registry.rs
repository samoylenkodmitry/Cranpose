//! The registry of an app's layout nodes: each node's parent and modifier
//! locals, through which a modifier chain resolves the locals its ancestors
//! provide.

use std::cell::RefCell;

use cranpose_core::{NodeId, collections::id_pages::IdPages};
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

struct LayoutNodeRegistryEntry {
    parent: Option<NodeId>,
    modifier_child_capabilities: NodeCapabilities,
    modifier_locals: Option<ModifierLocalsHandle>,
}

// The entries by node id: ids come from a counter, so the pages hold the
// nodes of a screen side by side.
pub(crate) struct LayoutNodeRegistryState {
    entries: RefCell<IdPages<Option<LayoutNodeRegistryEntry>>>,
}

impl LayoutNodeRegistryState {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::default(),
        }
    }

    fn register(&self, id: NodeId, node: &LayoutNode) {
        let entry = LayoutNodeRegistryEntry {
            parent: node.parent(),
            modifier_child_capabilities: node.modifier_child_capabilities(),
            modifier_locals: node.modifier_locals_handle(),
        };
        self.entries
            .borrow_mut()
            .update(id.0, |slot| *slot = Some(entry));
    }

    fn unregister(&self, id: NodeId) {
        self.entries.borrow_mut().update(id.0, |slot| *slot = None);
    }

    pub(super) fn update_entry(
        &self,
        id: NodeId,
        parent: Option<NodeId>,
        modifier_child_capabilities: NodeCapabilities,
        modifier_locals: Option<ModifierLocalsHandle>,
    ) {
        if let Some(entry) = self
            .entries
            .borrow_mut()
            .entry_mut(id.0)
            .and_then(Option::as_mut)
        {
            entry.parent = parent;
            entry.modifier_child_capabilities = modifier_child_capabilities;
            entry.modifier_locals = modifier_locals;
        }
    }

    #[cfg(test)]
    fn stats(&self) -> LayoutNodeRegistryDebugStats {
        let entries = self.entries.borrow();
        LayoutNodeRegistryDebugStats {
            len: entries.occupied(),
            capacity: entries.capacity(),
        }
    }

    fn resolve_modifier_local_from_parent_chain(
        &self,
        start: Option<NodeId>,
        token: &ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        let mut current = start;
        while let Some(parent_id) = current {
            let (next_parent, resolved) = {
                let entries = self.entries.borrow();
                if let Some(entry) = entries.entry(parent_id.0).and_then(Option::as_ref) {
                    let resolved = entry
                        .modifier_child_capabilities
                        .contains(NodeCapabilities::MODIFIER_LOCALS)
                        .then_some(entry.modifier_locals.as_ref())
                        .flatten()
                        .and_then(|locals| locals.borrow().resolve(token))
                        .map(|value| value.with_source(ModifierLocalSource::Ancestor));
                    (entry.parent, resolved)
                } else {
                    (None, None)
                }
            };
            if let Some(value) = resolved {
                return Some(value);
            }
            current = next_parent;
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
