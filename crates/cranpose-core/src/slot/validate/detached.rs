use super::super::DetachedSubtree;
use super::{
    anchors,
    groups::{validate_slot_tree, SlotTreeChecks, SlotTreeView},
    SlotInvariantError, SlotTreeContext,
};

struct DetachedSlotTreeChecks;

impl SlotTreeChecks for DetachedSlotTreeChecks {}

impl DetachedSubtree {
    pub(crate) fn validate_detached(&self) -> Result<(), SlotInvariantError> {
        let Some(root) = self.groups.first() else {
            return Err(SlotInvariantError::DetachedSubtreeEmpty);
        };
        let root_key = root.key;

        anchors::validate_detached_anchor_set(root_key, &self.groups)?;

        let mut checks = DetachedSlotTreeChecks;
        validate_slot_tree(
            SlotTreeView {
                tree: SlotTreeContext::Detached { root_key },
                groups: &self.groups,
                payloads: &self.payloads,
                nodes: &self.nodes,
            },
            &mut checks,
        )
    }
}
