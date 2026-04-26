use super::super::{AnchorState, GroupRecord, NodeLifecycle, NodeRecord, PayloadRecord, SlotTable};
use super::{tree::SlotTreeChecks, SlotInvariantError};
use crate::{collections::map::HashMap, slot_storage::GroupKey, AnchorId};

pub(super) struct ActiveSlotTreeChecks<'a> {
    table: &'a SlotTable,
    sibling_keys: HashMap<(AnchorId, GroupKey), usize>,
}

impl<'a> ActiveSlotTreeChecks<'a> {
    pub(super) fn new(table: &'a SlotTable) -> Self {
        Self {
            table,
            sibling_keys: HashMap::default(),
        }
    }
}

impl SlotTreeChecks for ActiveSlotTreeChecks<'_> {
    fn before_group(
        &mut self,
        group_index: usize,
        group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        match self.table.anchors.state(group.anchor) {
            Some(AnchorState::Active(actual)) if actual == group_index => Ok(()),
            actual => Err(SlotInvariantError::AnchorMismatch {
                anchor: group.anchor,
                expected: group_index,
                actual,
            }),
        }
    }

    fn after_group_header(
        &mut self,
        group_index: usize,
        group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        if self
            .sibling_keys
            .insert((group.parent_anchor, group.key), group_index)
            .is_some()
        {
            return Err(SlotInvariantError::DuplicateSiblingKey {
                parent_anchor: group.parent_anchor,
                key: group.key,
            });
        }
        Ok(())
    }

    fn validate_payload(
        &mut self,
        _group_index: usize,
        group: &GroupRecord,
        payload_index: usize,
        payload: &PayloadRecord,
    ) -> Result<(), SlotInvariantError> {
        let expected_location = (group.anchor, payload_index);
        let actual = self.table.payload_locations.get(payload.anchor);
        if actual != Some(expected_location) {
            return Err(SlotInvariantError::PayloadLocationMismatch {
                payload_anchor: payload.anchor,
                expected: expected_location,
                actual,
            });
        }
        Ok(())
    }

    fn after_payloads(
        &mut self,
        _group_index: usize,
        group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        if let Some(scope_id) = group.scope_id {
            let actual = self.table.scope_anchor_to_group.get(&scope_id).copied();
            if actual != Some(group.anchor) {
                return Err(SlotInvariantError::ScopeIndexMismatch {
                    scope_id,
                    expected: group.anchor,
                    actual,
                });
            }
        }
        Ok(())
    }

    fn validate_node(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
        node: &NodeRecord,
    ) -> Result<(), SlotInvariantError> {
        if node.lifecycle != NodeLifecycle::Active {
            return Err(SlotInvariantError::NodeLifecycleMismatch {
                node_id: node.id,
                expected: NodeLifecycle::Active,
                actual: node.lifecycle,
            });
        }
        Ok(())
    }
}
