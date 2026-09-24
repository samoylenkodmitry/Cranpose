use std::{cell::RefCell, collections::HashSet};

use cranpose_core::NodeId;

struct PointerDispatchManager {
    dirty_nodes: HashSet<NodeId>,
    is_processing: bool,
}

impl PointerDispatchManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
            is_processing: false,
        }
    }

    fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_repass(&self) -> bool {
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

pub(crate) struct PointerDispatchState {
    manager: RefCell<PointerDispatchManager>,
}

impl PointerDispatchState {
    pub(crate) fn new() -> Self {
        Self {
            manager: RefCell::new(PointerDispatchManager::new()),
        }
    }

    fn schedule_repass(&self, node_id: NodeId) {
        self.manager.borrow_mut().schedule_repass(node_id);
    }

    fn has_pending_repass(&self) -> bool {
        self.manager.borrow().has_pending_repass()
    }

    fn process_repasses<F>(&self, processor: F)
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

/// Schedules a pointer repass for the specified node.
///
/// This is called automatically when pointer modifiers invalidate
/// and mirrors Kotlin's `PointerInputDelegatingNode.requestPointerInput`.
pub fn schedule_pointer_repass(node_id: NodeId) {
    crate::render_state::with_pointer_dispatch(|state| state.schedule_repass(node_id));
}

/// Returns true if any pointer repasses are pending.
pub fn has_pending_pointer_repasses() -> bool {
    crate::render_state::with_pointer_dispatch(PointerDispatchState::has_pending_repass)
}

/// Processes all pending pointer repasses.
///
/// The host (e.g., app shell or layout engine) should call this after
/// composition/layout to service pointer invalidations without forcing
/// measure/layout passes.
pub fn process_pointer_repasses<F>(processor: F)
where
    F: FnMut(NodeId),
{
    crate::render_state::with_pointer_dispatch(|state| state.process_repasses(processor));
}

/// Clears all pending pointer repasses without processing them.
pub fn clear_pointer_repasses() {
    crate::render_state::with_pointer_dispatch(PointerDispatchState::clear);
}

#[cfg(test)]
#[path = "tests/pointer_dispatch_tests.rs"]
mod tests;
