use super::super::{checked_usize_to_u32, GroupRecord, NodeRecord, PayloadRecord, SlotTable};
use super::{
    anchors,
    nodes::{self, validate_group_nodes},
    payloads::{self, validate_group_payloads},
    scopes, SlotInvariantError, SlotTreeContext,
};
use crate::{
    collections::map::{HashMap, HashSet},
    slot_storage::GroupKey,
    AnchorId,
};

pub(super) struct SlotTreeView<'a> {
    pub(super) tree: SlotTreeContext,
    pub(super) groups: &'a [GroupRecord],
    pub(super) payloads: &'a [PayloadRecord],
    pub(super) nodes: &'a [NodeRecord],
}

pub(super) struct ActiveSlotTreeChecks<'a> {
    table: &'a SlotTable,
    sibling_keys: HashMap<(AnchorId, GroupKey), usize>,
    payload_anchors: HashSet<crate::slot_storage::PayloadAnchor>,
}

impl<'a> ActiveSlotTreeChecks<'a> {
    pub(super) fn new(table: &'a SlotTable) -> Self {
        Self {
            table,
            sibling_keys: HashMap::default(),
            payload_anchors: HashSet::default(),
        }
    }
}

impl SlotTreeView<'_> {
    fn invalid_parent(
        &self,
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        SlotInvariantError::InvalidParent {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    fn bad_depth(&self, group_index: usize, expected: u32, actual: u32) -> SlotInvariantError {
        SlotInvariantError::BadDepth {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    fn bad_subtree_len(
        &self,
        group_index: usize,
        expected: u32,
        actual: u32,
    ) -> SlotInvariantError {
        SlotInvariantError::BadSubtreeLen {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    fn bad_subtree_node_count(
        &self,
        group_index: usize,
        expected: u32,
        actual: u32,
    ) -> SlotInvariantError {
        SlotInvariantError::BadSubtreeNodeCount {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    pub(super) fn payload_start_mismatch(
        &self,
        group_index: usize,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::PayloadStartMismatch {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    pub(super) fn payload_out_of_range(
        &self,
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::PayloadOutOfRange {
            tree: self.tree,
            group_index,
            start,
            len,
            payload_count,
        }
    }

    pub(super) fn payload_count_mismatch(
        &self,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::PayloadCountMismatch {
            tree: self.tree,
            expected,
            actual,
        }
    }

    pub(super) fn payload_owner_mismatch(
        &self,
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        SlotInvariantError::PayloadOwnerMismatch {
            tree: self.tree,
            payload_anchor,
            expected,
            actual,
        }
    }

    pub(super) fn node_start_mismatch(
        &self,
        group_index: usize,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::NodeStartMismatch {
            tree: self.tree,
            group_index,
            expected,
            actual,
        }
    }

    pub(super) fn node_out_of_range(
        &self,
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    ) -> SlotInvariantError {
        SlotInvariantError::NodeOutOfRange {
            tree: self.tree,
            group_index,
            start,
            len,
            node_count,
        }
    }

    pub(super) fn node_count_mismatch(&self, expected: usize, actual: usize) -> SlotInvariantError {
        SlotInvariantError::NodeCountMismatch {
            tree: self.tree,
            expected,
            actual,
        }
    }

    pub(super) fn node_owner_mismatch(
        &self,
        node_id: crate::NodeId,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        SlotInvariantError::NodeOwnerMismatch {
            tree: self.tree,
            node_id,
            expected,
            actual,
        }
    }

    pub(super) fn duplicate_node_id(&self, node_id: crate::NodeId) -> SlotInvariantError {
        SlotInvariantError::DuplicateNodeId {
            tree: self.tree,
            node_id,
        }
    }
}

impl SlotTreeChecks for ActiveSlotTreeChecks<'_> {
    fn before_group(
        &mut self,
        group_index: usize,
        group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        anchors::validate_active_group_anchor(self.table, group_index, group)
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
        if !self.payload_anchors.insert(payload.anchor) {
            return Err(SlotInvariantError::DuplicatePayloadAnchor {
                tree: SlotTreeContext::Active,
                payload_anchor: payload.anchor,
            });
        }
        payloads::validate_active_payload_anchor(self.table, group, payload_index, payload)
    }

    fn after_payloads(
        &mut self,
        _group_index: usize,
        group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        scopes::validate_active_group_scope(self.table, group)
    }

    fn validate_node(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
        node: &NodeRecord,
    ) -> Result<(), SlotInvariantError> {
        nodes::validate_active_node_lifecycle(node)
    }
}

pub(super) trait SlotTreeChecks {
    fn before_group(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        Ok(())
    }

    fn after_group_header(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        Ok(())
    }

    fn validate_payload(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
        _payload_index: usize,
        _payload: &PayloadRecord,
    ) -> Result<(), SlotInvariantError> {
        Ok(())
    }

    fn after_payloads(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
    ) -> Result<(), SlotInvariantError> {
        Ok(())
    }

    fn validate_node(
        &mut self,
        _group_index: usize,
        _group: &GroupRecord,
        _node: &NodeRecord,
    ) -> Result<(), SlotInvariantError> {
        Ok(())
    }
}

pub(super) fn validate_slot_tree(
    view: SlotTreeView<'_>,
    checks: &mut impl SlotTreeChecks,
) -> Result<(), SlotInvariantError> {
    let mut stack: Vec<(AnchorId, usize)> = Vec::new();
    let mut anchor_to_group: HashMap<AnchorId, usize> = HashMap::default();
    let mut node_ids = HashSet::default();
    let mut expected_payload_start = 0usize;
    let mut expected_node_start = 0usize;

    for (index, group) in view.groups.iter().enumerate() {
        checks.before_group(index, group)?;
        anchor_to_group.insert(group.anchor, index);

        while let Some((_, end)) = stack.last() {
            if *end <= index {
                stack.pop();
            } else {
                break;
            }
        }

        let expected_parent = stack
            .last()
            .map(|(anchor, _)| *anchor)
            .unwrap_or(AnchorId::INVALID);
        if group.parent_anchor != expected_parent {
            return Err(view.invalid_parent(index, expected_parent, group.parent_anchor));
        }

        let expected_depth = checked_usize_to_u32(stack.len(), "validation group depth");
        if group.depth != expected_depth {
            return Err(view.bad_depth(index, expected_depth, group.depth));
        }

        let subtree_end = index
            .checked_add(group.subtree_len as usize)
            .unwrap_or_else(|| panic!("validation subtree end overflow at group {index}"));
        if subtree_end == index || subtree_end > view.groups.len() {
            return Err(view.bad_subtree_len(index, 0, group.subtree_len));
        }

        checks.after_group_header(index, group)?;

        expected_payload_start =
            validate_group_payloads(&view, checks, index, group, expected_payload_start)?;

        checks.after_payloads(index, group)?;

        expected_node_start = validate_group_nodes(
            &view,
            checks,
            &mut node_ids,
            index,
            group,
            expected_node_start,
        )?;

        stack.push((group.anchor, subtree_end));
    }

    if expected_payload_start != view.payloads.len() {
        return Err(view.payload_count_mismatch(expected_payload_start, view.payloads.len()));
    }
    if expected_node_start != view.nodes.len() {
        return Err(view.node_count_mismatch(expected_node_start, view.nodes.len()));
    }

    let mut expected_subtree_len = vec![1u32; view.groups.len()];
    let mut expected_subtree_node_count = view
        .groups
        .iter()
        .map(|group| group.node_len)
        .collect::<Vec<_>>();
    for index in (0..view.groups.len()).rev() {
        let parent_anchor = view.groups[index].parent_anchor;
        if !parent_anchor.is_valid() {
            continue;
        }
        let parent_index = anchor_to_group
            .get(&parent_anchor)
            .copied()
            .expect("validated parents must resolve");
        expected_subtree_len[parent_index] += expected_subtree_len[index];
        expected_subtree_node_count[parent_index] += expected_subtree_node_count[index];
    }

    for (index, group) in view.groups.iter().enumerate() {
        if group.subtree_len != expected_subtree_len[index] {
            return Err(view.bad_subtree_len(
                index,
                expected_subtree_len[index],
                group.subtree_len,
            ));
        }
        if group.subtree_node_count != expected_subtree_node_count[index] {
            return Err(view.bad_subtree_node_count(
                index,
                expected_subtree_node_count[index],
                group.subtree_node_count,
            ));
        }
    }

    Ok(())
}
