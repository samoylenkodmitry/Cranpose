//! Semantics invalidations raised from outside composition.
//!
//! A `.semantics()` recorder is re-run whenever the tree is collected, so a
//! recorder that reads live app state always reports the app's current answer.
//! What it could not do until now is say *when* that answer changed: the tree is
//! only re-collected when a node marks its semantics dirty, and the only things
//! that did so were attaching a node, updating its modifier chain (which needs a
//! recomposition) and a layout pass.
//!
//! An app whose screen is one `Canvas` has none of those. Its layout never
//! changes and its frame loop drives draws rather than recompositions, so the
//! semantics tree was published once — at boot, before there was anything on
//! screen — and never again. Every later screen inherited whatever the tree
//! happened to hold at the last pass, which for a screen reader means reading
//! out controls the user is no longer looking at.
//!
//! This is the missing half, and it mirrors Jetpack Compose's
//! `SemanticsModifierNode.invalidateSemantics()`: a node can be marked for
//! re-collection directly, with no recomposition and no layout pass. The queue
//! is drained by the app shell each frame with the applier in hand, which is
//! where the dirty flag can actually be bubbled to the root that
//! [`crate::tree_needs_semantics`] reads.
//!
//! Structured like [`crate::focus_dispatch`], for the same reason: the request
//! arrives on the app's own thread at an arbitrary moment, and the tree may only
//! be touched at a defined point in the frame.

use std::{cell::RefCell, collections::HashSet};

use cranpose_core::NodeId;

/// Layout nodes whose semantics need re-collecting.
struct SemanticsInvalidationManager {
    dirty_nodes: HashSet<NodeId>,
    is_processing: bool,
}

impl SemanticsInvalidationManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
            is_processing: false,
        }
    }

    fn schedule_invalidation(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn take_pending_for_processing(&mut self) -> Option<Vec<NodeId>> {
        if self.is_processing {
            return None;
        }

        self.is_processing = true;
        Some(self.dirty_nodes.drain().collect())
    }

    fn finish_processing<I>(&mut self, remaining: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        self.dirty_nodes.extend(remaining);
        self.is_processing = false;
    }

    fn clear(&mut self) {
        self.dirty_nodes.clear();
    }
}

pub(crate) struct SemanticsInvalidationState {
    manager: RefCell<SemanticsInvalidationManager>,
}

impl SemanticsInvalidationState {
    pub(crate) fn new() -> Self {
        Self {
            manager: RefCell::new(SemanticsInvalidationManager::new()),
        }
    }

    fn schedule_invalidation(&self, node_id: NodeId) {
        self.manager.borrow_mut().schedule_invalidation(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        self.manager.borrow().has_pending_invalidation()
    }

    fn process_invalidations<F>(&self, processor: F)
    where
        F: FnMut(NodeId),
    {
        let Some(nodes) = self.manager.borrow_mut().take_pending_for_processing() else {
            return;
        };

        self.process_pending_nodes(nodes, processor);
    }

    fn clear(&self) {
        self.manager.borrow_mut().clear();
    }

    fn process_pending_nodes<F>(&self, nodes: Vec<NodeId>, mut processor: F)
    where
        F: FnMut(NodeId),
    {
        let mut remaining = nodes.into_iter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for node_id in remaining.by_ref() {
                processor(node_id);
            }
        }));

        self.manager.borrow_mut().finish_processing(remaining);

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Marks `node_id`'s semantics for re-collection on the next frame of the app
/// context that is current.
///
/// [`crate::SemanticsRequester::invalidate`] uses the by-id form instead: it
/// remembers the context its node attached in, so a request raised from a frame
/// loop or an event callback outside any context still reaches the right queue.
pub fn schedule_semantics_invalidation(node_id: NodeId) {
    crate::render_state::with_semantics_dispatch(|state| {
        state.schedule_invalidation(node_id);
    });
}

pub(crate) fn schedule_semantics_invalidation_in(
    app_context: crate::render_state::AppContextId,
    node_id: NodeId,
) {
    crate::render_state::with_semantics_dispatch_by_app_context(app_context, |state| {
        state.schedule_invalidation(node_id);
    });
}

/// Whether any semantics invalidations are waiting to be serviced.
pub fn has_pending_semantics_invalidations() -> bool {
    crate::render_state::with_semantics_dispatch(|state| state.has_pending_invalidation())
}

/// Services every pending semantics invalidation.
///
/// The host calls this once per frame with the applier available, and bubbles
/// the dirty flag from each node to the root.
pub fn process_semantics_invalidations<F>(processor: F)
where
    F: FnMut(NodeId),
{
    crate::render_state::with_semantics_dispatch(|state| state.process_invalidations(processor));
}

/// Drops every pending semantics invalidation without servicing it.
pub fn clear_semantics_invalidations() {
    crate::render_state::with_semantics_dispatch(|state| state.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheduled_node_is_handed_to_the_processor_once() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_semantics_invalidations();

        schedule_semantics_invalidation(7);
        schedule_semantics_invalidation(7);
        schedule_semantics_invalidation(9);
        assert!(has_pending_semantics_invalidations());

        let mut seen = Vec::new();
        process_semantics_invalidations(|node_id| seen.push(node_id));
        seen.sort_unstable();
        assert_eq!(seen, vec![7, 9]);
        assert!(!has_pending_semantics_invalidations());
    }

    #[test]
    fn nothing_is_pending_until_something_asks() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_semantics_invalidations();
        assert!(!has_pending_semantics_invalidations());

        let mut seen = 0;
        process_semantics_invalidations(|_| seen += 1);
        assert_eq!(seen, 0);
    }
}
