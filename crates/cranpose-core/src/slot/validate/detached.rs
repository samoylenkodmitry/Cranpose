use super::{
    super::{DetachedSubtree, GroupKey, GroupRecord, PayloadAnchor, PayloadRecord},
    anchors,
    groups::{validate_slot_tree, SlotTreeChecks, SlotTreeView},
    SlotInvariantError, SlotTreeContext,
};
use crate::collections::map::HashSet;

struct DetachedSlotTreeChecks {
    root_key: GroupKey,
    payload_anchors: HashSet<PayloadAnchor>,
}

impl DetachedSlotTreeChecks {
    fn new(root_key: GroupKey) -> Self {
        Self {
            root_key,
            payload_anchors: HashSet::default(),
        }
    }
}

impl SlotTreeChecks for DetachedSlotTreeChecks {
    fn validate_payload(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
        _payload_index: usize,
        payload: &PayloadRecord,
    ) -> Result<(), SlotInvariantError> {
        if self.payload_anchors.insert(payload.anchor) {
            return Ok(());
        }
        Err(SlotInvariantError::DuplicatePayloadAnchor {
            tree: SlotTreeContext::Detached {
                root_key: self.root_key,
            },
            payload_anchor: payload.anchor,
        })
    }
}

impl DetachedSubtree {
    pub(crate) fn validate_detached(&self) -> Result<(), SlotInvariantError> {
        let Some(root) = self.groups.first() else {
            return Err(SlotInvariantError::DetachedSubtreeEmpty);
        };
        let root_key = root.key;

        anchors::validate_detached_anchor_set(root_key, &self.groups)?;

        let mut checks = DetachedSlotTreeChecks::new(root_key);
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
