//! Pointer input dispatch manager for Compose-RS.
//!
//! This module implements pointer invalidation servicing that mirrors Jetpack Compose's
//! pointer input invalidation system. When pointer modifiers change, they mark nodes
//! for reprocessing without forcing layout/draw passes.

use compose_core::NodeId;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static POINTER_DISPATCH_MANAGER: RefCell<PointerDispatchManager> =
        RefCell::new(PointerDispatchManager::new());
}

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

    fn process_repasses<F>(&mut self, mut processor: F)
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

/// Schedules a pointer repass for the specified node.
///
/// This is called automatically when pointer modifiers invalidate
/// and mirrors Kotlin's `PointerInputDelegatingNode.requestPointerInput`.
pub fn schedule_pointer_repass(node_id: NodeId) {
    POINTER_DISPATCH_MANAGER.with(|manager| {
        manager.borrow_mut().schedule_repass(node_id);
    });
}

/// Returns true if any pointer repasses are pending.
pub fn has_pending_pointer_repasses() -> bool {
    POINTER_DISPATCH_MANAGER.with(|manager| manager.borrow().has_pending_repass())
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
    POINTER_DISPATCH_MANAGER.with(|manager| {
        manager.borrow_mut().process_repasses(processor);
    });
}

/// Clears all pending pointer repasses without processing them.
pub fn clear_pointer_repasses() {
    POINTER_DISPATCH_MANAGER.with(|manager| {
        manager.borrow_mut().clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_and_process_repasses() {
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
}
