use super::super::DetachedSubtree;
use super::{
    tree::{validate_slot_tree, SlotTreeChecks, SlotTreeKind, SlotTreeView},
    SlotInvariantError,
};
use crate::{collections::map::HashSet, AnchorId};

struct DetachedSlotTreeChecks;

impl SlotTreeChecks for DetachedSlotTreeChecks {}

impl DetachedSubtree {
    pub(crate) fn validate_detached(&self) -> Result<(), SlotInvariantError> {
        let Some(root) = self.groups.first() else {
            return Err(SlotInvariantError::DetachedSubtreeEmpty);
        };
        let root_key = root.key;

        let mut anchor_set: HashSet<AnchorId> = HashSet::default();
        for anchor in self.groups.iter().map(|group| group.anchor) {
            if !anchor_set.insert(anchor) {
                return Err(SlotInvariantError::DetachedDuplicateAnchor { root_key, anchor });
            }
        }

        let mut checks = DetachedSlotTreeChecks;
        validate_slot_tree(
            SlotTreeView {
                kind: SlotTreeKind::Detached { root_key },
                groups: &self.groups,
                payloads: &self.payloads,
                nodes: &self.nodes,
            },
            &mut checks,
        )
    }
}
