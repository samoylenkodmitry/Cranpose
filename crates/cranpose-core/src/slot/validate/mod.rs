#[cfg(any(test, debug_assertions))]
mod anchors;
#[cfg(any(test, debug_assertions))]
mod detached;
#[cfg(any(test, debug_assertions))]
mod errors;
#[cfg(any(test, debug_assertions))]
mod groups;
#[cfg(any(test, debug_assertions))]
mod nodes;
#[cfg(any(test, debug_assertions))]
mod payloads;
#[cfg(any(test, debug_assertions))]
mod scopes;
#[cfg(any(test, debug_assertions))]
mod writer;

#[cfg(any(test, debug_assertions))]
pub(crate) use errors::{PayloadAnchorRecord, SlotInvariantError, SlotTreeContext};
#[cfg(any(test, debug_assertions))]
use groups::{ActiveSlotTreeChecks, SlotTreeView, validate_slot_tree};

use super::SlotTable;

impl SlotTable {
    #[track_caller]
    pub(crate) fn assert_fast_integrity(&self, context: &'static str) {
        assert_eq!(
            self.anchors.active_len(),
            self.groups.len(),
            "slot table active group anchor count mismatch after {context}"
        );
        assert_eq!(
            self.payload_anchors.active_len(),
            self.payloads.len(),
            "slot table active payload anchor count mismatch after {context}"
        );
        assert!(
            self.scope_index.len() <= self.groups.len(),
            "slot table scope index has more entries than active groups after {context}"
        );
    }
}

#[cfg(any(test, debug_assertions))]
impl SlotTable {
    pub(crate) fn debug_verify(&self) {
        if crate::slot_validation_diagnostics_enabled()
            && let Err(err) = self.validate()
        {
            panic!("slot table invariant violation: {err:?}");
        }
    }

    pub(in crate::slot) fn debug_assert_valid_after(&self, operation: &'static str) {
        if crate::slot_validation_diagnostics_enabled() {
            if let Err(err) = self.validate() {
                panic!("slot table invariant violation after {operation}: {err:?}");
            }
        } else {
            self.assert_fast_integrity(operation);
        }
    }

    pub(crate) fn validate(&self) -> Result<(), SlotInvariantError> {
        let mut checks = ActiveSlotTreeChecks::new(self);
        validate_slot_tree(
            SlotTreeView {
                tree: SlotTreeContext::Active,
                groups: &self.groups,
                payloads: &self.payloads,
                nodes: &self.nodes,
            },
            &mut checks,
        )?;

        anchors::validate_anchor_registry_integrity(self)?;
        anchors::validate_active_group_anchor_count(self)?;
        anchors::validate_active_group_anchor_entries(self)?;
        payloads::validate_payload_anchor_registry_integrity(self)?;
        payloads::validate_payload_anchor_registry_count(self)?;
        scopes::validate_scope_index_count(self)?;
        payloads::validate_payload_anchor_registry(self)?;

        Ok(())
    }
}
