//! The record of the states each node's measure read in an app context, which
//! measures the node again when one of them changes.

use std::rc::Rc;

use cranpose_core::{NodeId, internal::NodeReads};

use super::{AppContextId, with_app_context_by_id};

// The measure reads of the context `context_id`.
pub(super) fn new(context_id: AppContextId) -> Rc<NodeReads> {
    NodeReads::new(move |nodes| {
        with_app_context_by_id(context_id, |context| {
            let mut repasses = context.state.measure_repasses.borrow_mut();
            for &node_id in nodes {
                repasses.schedule_repass(node_id);
            }
            drop(repasses);
            context.state.render_invalidated.set(true);
        });
    })
}

// Drops the reads of `node_id`, which leaves the context `context_id`.
pub(crate) fn forget(context_id: AppContextId, node_id: NodeId) {
    with_app_context_by_id(context_id, |context| context.measure_reads.forget(node_id));
}
