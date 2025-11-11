//! Focus invalidation manager for Compose-RS.
//!
//! This module implements focus invalidation servicing that mirrors Jetpack Compose's
//! `FocusInvalidationManager`. When focus modifiers change, they mark nodes for
//! reprocessing without forcing layout/draw passes.

use compose_core::NodeId;
use compose_foundation::{FocusNode, FocusState};
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static FOCUS_INVALIDATION_MANAGER: RefCell<FocusInvalidationManager> =
        RefCell::new(FocusInvalidationManager::new());
}

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

    fn process_invalidations<F>(&mut self, mut processor: F)
    where
        F: FnMut(NodeId),
    {
        if self.is_processing {
            return;
        }

        self.is_processing = true;

        // Process all dirty nodes
        let nodes: Vec<NodeId> = self.dirty_nodes.drain().collect();
        for node_id in nodes {
            processor(node_id);
        }

        self.is_processing = false;
    }

    fn clear(&mut self) {
        self.dirty_nodes.clear();
    }
}

/// Schedules a focus invalidation for the specified node.
///
/// This is called automatically when focus modifiers invalidate
/// and mirrors Kotlin's `FocusInvalidationManager.scheduleInvalidation`.
pub fn schedule_focus_invalidation(node_id: NodeId) {
    FOCUS_INVALIDATION_MANAGER.with(|manager| {
        manager.borrow_mut().schedule_invalidation(node_id);
    });
}

/// Returns true if any focus invalidations are pending.
pub fn has_pending_focus_invalidations() -> bool {
    FOCUS_INVALIDATION_MANAGER.with(|manager| manager.borrow().has_pending_invalidation())
}

/// Sets the currently active focus target.
///
/// This mirrors Kotlin's `FocusOwner.activeFocusTargetNode` and allows
/// the focus system to track which node currently has focus.
pub fn set_active_focus_target(node_id: Option<NodeId>) {
    FOCUS_INVALIDATION_MANAGER.with(|manager| {
        manager.borrow_mut().set_active_focus_target(node_id);
    });
}

/// Returns the currently active focus target, if any.
pub fn active_focus_target() -> Option<NodeId> {
    FOCUS_INVALIDATION_MANAGER.with(|manager| manager.borrow().active_focus_target())
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
    FOCUS_INVALIDATION_MANAGER.with(|manager| {
        manager.borrow_mut().process_invalidations(processor);
    });
}

/// Clears all pending focus invalidations without processing them.
pub fn clear_focus_invalidations() {
    FOCUS_INVALIDATION_MANAGER.with(|manager| {
        manager.borrow_mut().clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_process_invalidations() {
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
}
