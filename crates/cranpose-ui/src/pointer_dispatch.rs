use cranpose_core::NodeId;

use crate::dirty_node_queue::DirtyNodeQueue;

/// Schedules a pointer repass for the specified node.
///
/// This is called automatically when pointer modifiers invalidate
/// and mirrors Kotlin's `PointerInputDelegatingNode.requestPointerInput`.
pub fn schedule_pointer_repass(node_id: NodeId) {
    crate::render_state::with_pointer_dispatch(|queue| queue.schedule(node_id));
}

/// Returns true if any pointer repasses are pending.
pub fn has_pending_pointer_repasses() -> bool {
    crate::render_state::require_current_app_context("pointer dispatch access")
        .has_pending_pointer_repasses()
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
    crate::render_state::with_pointer_dispatch(|queue| queue.process(processor));
}

/// Clears all pending pointer repasses without processing them.
pub fn clear_pointer_repasses() {
    crate::render_state::with_pointer_dispatch(DirtyNodeQueue::clear);
}
