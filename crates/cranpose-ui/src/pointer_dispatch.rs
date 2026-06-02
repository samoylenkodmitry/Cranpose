//! Pointer input dispatch manager for Cranpose.
//!
//! This module manages pointer input invalidations across the UI tree.
//! Hit path tracking for gesture state preservation is handled by
//! `AppShell::cached_hits` which caches hit targets on pointer DOWN
//! and dispatches subsequent MOVE/UP events to the same cached nodes.

use cranpose_core::NodeId;
use std::cell::RefCell;
use std::collections::HashSet;

// ============================================================================
// PointerDispatchManager - Invalidation tracking
// ============================================================================

/// Manages pointer input invalidations across the UI tree.
///
/// Similar to Kotlin's pointer input invalidation system, this tracks
/// which layout nodes need pointer input reprocessing and provides
/// hooks for the runtime to service those invalidations.
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
    crate::render_state::with_pointer_dispatch(|state| state.has_pending_repass())
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
    crate::render_state::with_pointer_dispatch(|state| state.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_process_repasses() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_pointer_repasses();

        let node1: NodeId = 1;
        let node2: NodeId = 2;

        schedule_pointer_repass(node1);
        schedule_pointer_repass(node2);

        assert!(has_pending_pointer_repasses());

        let mut processed = Vec::new();
        process_pointer_repasses(|node_id| {
            processed.push(node_id);
        });

        assert_eq!(processed.len(), 2);
        assert!(processed.contains(&node1));
        assert!(processed.contains(&node2));
        assert!(!has_pending_pointer_repasses());
    }

    #[test]
    fn duplicate_schedules_deduplicated() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_pointer_repasses();

        let node: NodeId = 42;
        schedule_pointer_repass(node);
        schedule_pointer_repass(node);
        schedule_pointer_repass(node);

        let mut count = 0;
        process_pointer_repasses(|_| {
            count += 1;
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn process_repasses_recovers_after_processor_panic() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_pointer_repasses();

        schedule_pointer_repass(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_pointer_repasses(|_| panic!("pointer repass processor panic"));
        }));
        assert!(result.is_err());

        schedule_pointer_repass(2);
        let mut processed = Vec::new();
        process_pointer_repasses(|node_id| processed.push(node_id));

        assert!(
            processed.contains(&2),
            "pointer repass processing must not stay stuck after a processor panic"
        );
        assert!(!has_pending_pointer_repasses());
    }

    #[test]
    fn process_repasses_allows_processor_to_schedule_more_work() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_pointer_repasses();

        schedule_pointer_repass(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_pointer_repasses(|_| schedule_pointer_repass(2));
        }));
        assert!(
            result.is_ok(),
            "pointer repass processors must be able to enqueue follow-up repasses"
        );
        assert!(has_pending_pointer_repasses());

        let mut processed = Vec::new();
        process_pointer_repasses(|node_id| processed.push(node_id));

        assert_eq!(processed, vec![2]);
        assert!(!has_pending_pointer_repasses());
    }

    #[test]
    fn pointer_repasses_are_scoped_by_app_context() {
        let _app_context = crate::render_state::app_context_test_scope();
        let first = crate::render_state::AppContext::new_with_density(1.0);
        let second = crate::render_state::AppContext::new_with_density(1.0);

        first.enter(|| {
            clear_pointer_repasses();
            schedule_pointer_repass(7);
            assert!(has_pending_pointer_repasses());
        });

        second.enter(|| {
            clear_pointer_repasses();
            assert!(!has_pending_pointer_repasses());
            schedule_pointer_repass(9);
        });

        first.enter(|| {
            let mut processed = Vec::new();
            process_pointer_repasses(|node_id| processed.push(node_id));
            assert_eq!(processed, vec![7]);
        });

        second.enter(|| {
            let mut processed = Vec::new();
            process_pointer_repasses(|node_id| processed.push(node_id));
            assert_eq!(processed, vec![9]);
        });
    }
}
