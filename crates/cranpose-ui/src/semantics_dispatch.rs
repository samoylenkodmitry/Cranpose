use cranpose_core::NodeId;

use crate::dirty_node_queue::DirtyNodeQueue;

/// Marks `node_id`'s semantics for re-collection on the next frame of the app
/// context that is current.
///
/// [`crate::SemanticsRequester::invalidate`] uses the by-id form instead: it
/// remembers the context its node attached in, so a request raised from a frame
/// loop or an event callback outside any context still reaches the right queue.
pub fn schedule_semantics_invalidation(node_id: NodeId) {
    crate::render_state::with_semantics_dispatch(|queue| queue.schedule(node_id));
}

pub(crate) fn schedule_semantics_invalidation_in(
    app_context: crate::render_state::AppContextId,
    node_id: NodeId,
) {
    crate::render_state::with_semantics_dispatch_by_app_context(app_context, |queue| {
        queue.schedule(node_id);
    });
}

/// Whether any semantics invalidations are waiting to be serviced.
pub fn has_pending_semantics_invalidations() -> bool {
    crate::render_state::require_current_app_context("semantics dispatch access")
        .has_pending_semantics_invalidations()
}

/// Services every pending semantics invalidation.
///
/// The host calls this once per frame with the applier available, and bubbles
/// the dirty flag from each node to the root.
pub fn process_semantics_invalidations<F>(processor: F)
where
    F: FnMut(NodeId),
{
    crate::render_state::with_semantics_dispatch(|queue| queue.process(processor));
}

/// Drops every pending semantics invalidation without servicing it.
pub fn clear_semantics_invalidations() {
    crate::render_state::with_semantics_dispatch(DirtyNodeQueue::clear);
}
