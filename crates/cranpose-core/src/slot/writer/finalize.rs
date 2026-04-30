use super::super::{
    detach::{dispose_detached_node_now, dispose_detached_subtree_now},
    DetachedSubtree, SlotWriteSession,
};
use crate::{Applier, NodeError};

impl SlotWriteSession<'_> {
    pub(crate) fn finalize_pass(
        &mut self,
        applier: &mut dyn Applier,
    ) -> Result<Vec<DetachedSubtree>, NodeError> {
        while !self.state.group_stack.is_empty() {
            let result = self.finish_group_body();
            for subtree in result.detached_children {
                self.table.invalidate_detached_subtree_anchors(&subtree);
                dispose_detached_subtree_now(applier, &subtree)?;
                self.lifecycle.queue_subtree_disposal(subtree);
            }
            for node_id in result.direct_nodes {
                dispose_detached_node_now(applier, node_id)?;
            }
            self.end_group();
        }
        self.flush_payload_location_refreshes();
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_no_pending_payload_location_refreshes("finalize_pass");

        let root_detached = self.table.root_finish_result(self.state);
        self.state.note_detached_subtrees(&root_detached);
        #[cfg(any(test, debug_assertions))]
        self.state
            .debug_assert_valid_after(self.table, "finalize_pass");
        Ok(root_detached)
    }
}
