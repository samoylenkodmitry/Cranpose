//! Focus invalidation manager for Cranpose.
//!
//! This module implements focus invalidation servicing that mirrors Jetpack Compose's
//! `FocusInvalidationManager`. When focus modifiers change, they mark nodes for
//! reprocessing without forcing layout/draw passes.

use cranpose_core::NodeId;
use std::cell::RefCell;
use std::collections::HashSet;

/// Manages focus invalidations across the UI tree.
///
/// Similar to Kotlin's `FocusInvalidationManager`, this tracks which
/// layout nodes need focus state reprocessing and provides hooks for
/// the runtime to service those invalidations.
struct FocusInvalidationManager {
    dirty_nodes: HashSet<NodeId>,
    is_processing: bool,
    active_focus_target: Option<NodeId>,
}

impl FocusInvalidationManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
            is_processing: false,
            active_focus_target: None,
        }
    }

    fn schedule_invalidation(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn set_active_focus_target(&mut self, node_id: Option<NodeId>) {
        self.active_focus_target = node_id;
    }

    fn active_focus_target(&self) -> Option<NodeId> {
        self.active_focus_target
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

pub(crate) struct FocusInvalidationState {
    manager: RefCell<FocusInvalidationManager>,
}

impl FocusInvalidationState {
    pub(crate) fn new() -> Self {
        Self {
            manager: RefCell::new(FocusInvalidationManager::new()),
        }
    }

    fn schedule_invalidation(&self, node_id: NodeId) {
        self.manager.borrow_mut().schedule_invalidation(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        self.manager.borrow().has_pending_invalidation()
    }

    fn set_active_focus_target(&self, node_id: Option<NodeId>) {
        self.manager.borrow_mut().set_active_focus_target(node_id);
    }

    fn active_focus_target(&self) -> Option<NodeId> {
        self.manager.borrow().active_focus_target()
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

/// Schedules a focus invalidation for the specified node.
///
/// This is called automatically when focus modifiers invalidate
/// and mirrors Kotlin's `FocusInvalidationManager.scheduleInvalidation`.
pub fn schedule_focus_invalidation(node_id: NodeId) {
    crate::render_state::with_focus_dispatch(|state| state.schedule_invalidation(node_id));
}

/// Returns true if any focus invalidations are pending.
pub fn has_pending_focus_invalidations() -> bool {
    crate::render_state::with_focus_dispatch(|state| state.has_pending_invalidation())
}

/// Sets the currently active focus target.
///
/// This mirrors Kotlin's `FocusOwner.activeFocusTargetNode` and allows
/// the focus system to track which node currently has focus.
pub fn set_active_focus_target(node_id: Option<NodeId>) {
    crate::render_state::with_focus_dispatch(|state| state.set_active_focus_target(node_id));
}

/// Returns the currently active focus target, if any.
pub fn active_focus_target() -> Option<NodeId> {
    crate::render_state::with_focus_dispatch(|state| state.active_focus_target())
}

/// Processes all pending focus invalidations.
///
/// The host (e.g., app shell or layout engine) should call this after
/// composition/layout to service focus invalidations without forcing
/// measure/layout passes.
pub fn process_focus_invalidations<F>(processor: F)
where
    F: FnMut(NodeId),
{
    crate::render_state::with_focus_dispatch(|state| state.process_invalidations(processor));
}

/// Clears all pending focus invalidations without processing them.
pub fn clear_focus_invalidations() {
    crate::render_state::with_focus_dispatch(|state| state.clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_process_invalidations() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_focus_invalidations();

        let node1: NodeId = 1;
        let node2: NodeId = 2;

        schedule_focus_invalidation(node1);
        schedule_focus_invalidation(node2);

        assert!(has_pending_focus_invalidations());

        let mut processed = Vec::new();
        process_focus_invalidations(|node_id| {
            processed.push(node_id);
        });

        assert_eq!(processed.len(), 2);
        assert!(processed.contains(&node1));
        assert!(processed.contains(&node2));
        assert!(!has_pending_focus_invalidations());
    }

    #[test]
    fn active_focus_target_tracking() {
        let _app_context = crate::render_state::app_context_test_scope();
        set_active_focus_target(None);
        assert_eq!(active_focus_target(), None);

        let node: NodeId = 42;
        set_active_focus_target(Some(node));
        assert_eq!(active_focus_target(), Some(node));

        set_active_focus_target(None);
        assert_eq!(active_focus_target(), None);
    }

    #[test]
    fn duplicate_invalidations_deduplicated() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_focus_invalidations();

        let node: NodeId = 42;
        schedule_focus_invalidation(node);
        schedule_focus_invalidation(node);
        schedule_focus_invalidation(node);

        let mut count = 0;
        process_focus_invalidations(|_| {
            count += 1;
        });

        assert_eq!(count, 1);
    }

    #[test]
    fn process_invalidations_recovers_after_processor_panic() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_focus_invalidations();

        schedule_focus_invalidation(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_focus_invalidations(|_| panic!("focus processor panic"));
        }));
        assert!(result.is_err());

        schedule_focus_invalidation(2);
        let mut processed = Vec::new();
        process_focus_invalidations(|node_id| processed.push(node_id));

        assert!(
            processed.contains(&2),
            "focus invalidation processing must not stay stuck after a processor panic"
        );
        assert!(!has_pending_focus_invalidations());
    }

    #[test]
    fn process_invalidations_allows_processor_to_schedule_more_work() {
        let _app_context = crate::render_state::app_context_test_scope();
        clear_focus_invalidations();

        schedule_focus_invalidation(1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            process_focus_invalidations(|_| schedule_focus_invalidation(2));
        }));
        assert!(
            result.is_ok(),
            "focus processors must be able to enqueue follow-up invalidations"
        );
        assert!(has_pending_focus_invalidations());

        let mut processed = Vec::new();
        process_focus_invalidations(|node_id| processed.push(node_id));

        assert_eq!(processed, vec![2]);
        assert!(!has_pending_focus_invalidations());
    }

    #[test]
    fn focus_state_is_scoped_by_app_context() {
        let _app_context = crate::render_state::app_context_test_scope();
        let first = crate::render_state::AppContext::new_with_density(1.0);
        let second = crate::render_state::AppContext::new_with_density(1.0);

        first.enter(|| {
            clear_focus_invalidations();
            schedule_focus_invalidation(7);
            set_active_focus_target(Some(17));
            assert!(has_pending_focus_invalidations());
            assert_eq!(active_focus_target(), Some(17));
        });

        second.enter(|| {
            clear_focus_invalidations();
            assert!(!has_pending_focus_invalidations());
            assert_eq!(active_focus_target(), None);
            schedule_focus_invalidation(9);
            set_active_focus_target(Some(19));
        });

        first.enter(|| {
            let mut processed = Vec::new();
            process_focus_invalidations(|node_id| processed.push(node_id));
            assert_eq!(processed, vec![7]);
            assert_eq!(active_focus_target(), Some(17));
        });

        second.enter(|| {
            let mut processed = Vec::new();
            process_focus_invalidations(|node_id| processed.push(node_id));
            assert_eq!(processed, vec![9]);
            assert_eq!(active_focus_target(), Some(19));
        });
    }
}
