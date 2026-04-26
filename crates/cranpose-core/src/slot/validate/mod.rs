#[cfg(any(test, debug_assertions))]
mod active;
#[cfg(any(test, debug_assertions))]
mod detached;
#[cfg(any(test, debug_assertions))]
mod errors;
#[cfg(any(test, debug_assertions))]
mod nodes;
#[cfg(any(test, debug_assertions))]
mod payloads;
#[cfg(any(test, debug_assertions))]
mod tree;

#[cfg(any(test, debug_assertions))]
use super::SlotTable;
#[cfg(any(test, debug_assertions))]
use active::ActiveSlotTreeChecks;
#[cfg(any(test, debug_assertions))]
pub(crate) use errors::SlotInvariantError;
#[cfg(any(test, debug_assertions))]
use tree::{validate_slot_tree, SlotTreeKind, SlotTreeView};

#[cfg(any(test, debug_assertions))]
impl SlotTable {
    pub(crate) fn debug_verify(&self) {
        if crate::slot_validation_diagnostics_enabled() {
            if let Err(err) = self.validate() {
                panic!("slot table invariant violation: {err:?}");
            }
        }
    }

    pub(in crate::slot) fn debug_assert_valid_after(&self, operation: &'static str) {
        if let Err(err) = self.validate() {
            panic!("slot table invariant violation after {operation}: {err:?}");
        }
    }

    pub(crate) fn validate(&self) -> Result<(), SlotInvariantError> {
        let scope_count = self
            .groups
            .iter()
            .filter(|group| group.scope_id.is_some())
            .count();

        let mut checks = ActiveSlotTreeChecks::new(self);
        validate_slot_tree(
            SlotTreeView {
                kind: SlotTreeKind::Active,
                groups: &self.groups,
                payloads: &self.payloads,
                nodes: &self.nodes,
            },
            &mut checks,
        )?;

        if self.anchors.active_len() != self.groups.len() {
            return Err(SlotInvariantError::GroupAnchorCountMismatch {
                expected: self.groups.len(),
                actual: self.anchors.active_len(),
            });
        }

        if self.payload_locations.len() != self.payloads.len() {
            return Err(SlotInvariantError::PayloadAnchorCountMismatch {
                expected: self.payloads.len(),
                actual: self.payload_locations.len(),
            });
        }

        if self.scope_anchor_to_group.len() != scope_count {
            return Err(SlotInvariantError::ScopeIndexCountMismatch {
                expected: scope_count,
                actual: self.scope_anchor_to_group.len(),
            });
        }

        for (payload_anchor, (owner, payload_index)) in self.payload_locations.iter() {
            let Some(group_index) = self.anchors.active_index(owner) else {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: None,
                });
            };
            let actual = self
                .group_payload_records_at(group_index)
                .get(payload_index)
                .map(|payload| (payload.owner, payload.anchor));
            if actual != Some((owner, payload_anchor)) {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: Some((owner, payload_index)),
                });
            }
        }

        Ok(())
    }
}
