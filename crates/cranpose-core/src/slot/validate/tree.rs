use super::super::{GroupRecord, NodeRecord, PayloadRecord};
use super::SlotInvariantError;
use super::{nodes::validate_group_nodes, payloads::validate_group_payloads};
use crate::{
    collections::map::{HashMap, HashSet},
    slot_storage::GroupKey,
    AnchorId,
};

pub(super) struct SlotTreeView<'a> {
    pub(super) kind: SlotTreeKind,
    pub(super) groups: &'a [GroupRecord],
    pub(super) payloads: &'a [PayloadRecord],
    pub(super) nodes: &'a [NodeRecord],
}

#[derive(Clone, Copy)]
pub(super) enum SlotTreeKind {
    Active,
    Detached { root_key: GroupKey },
}

impl SlotTreeKind {
    fn invalid_parent(
        self,
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::InvalidParent {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedInvalidParent {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    fn bad_depth(self, group_index: usize, expected: u32, actual: u32) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::BadDepth {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedBadDepth {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    fn bad_subtree_len(self, group_index: usize, expected: u32, actual: u32) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::BadSubtreeLen {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedBadSubtreeLen {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    fn bad_subtree_node_count(
        self,
        group_index: usize,
        expected: u32,
        actual: u32,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::BadSubtreeNodeCount {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedBadSubtreeNodeCount {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    pub(super) fn payload_start_mismatch(
        self,
        group_index: usize,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::PayloadStartMismatch {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedPayloadStartMismatch {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    pub(super) fn payload_out_of_range(
        self,
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::PayloadOutOfRange {
                group_index,
                start,
                len,
                payload_count,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedPayloadOutOfRange {
                root_key,
                group_index,
                start,
                len,
                payload_count,
            },
        }
    }

    pub(super) fn payload_count_mismatch(
        self,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::PayloadCountMismatch { expected, actual },
            Self::Detached { root_key } => SlotInvariantError::DetachedPayloadCountMismatch {
                root_key,
                expected,
                actual,
            },
        }
    }

    pub(super) fn payload_owner_mismatch(
        self,
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::PayloadOwnerMismatch {
                payload_anchor,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedPayloadOwnerMismatch {
                root_key,
                payload_anchor,
                expected,
                actual,
            },
        }
    }

    pub(super) fn node_start_mismatch(
        self,
        group_index: usize,
        expected: usize,
        actual: usize,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::NodeStartMismatch {
                group_index,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedNodeStartMismatch {
                root_key,
                group_index,
                expected,
                actual,
            },
        }
    }

    pub(super) fn node_out_of_range(
        self,
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::NodeOutOfRange {
                group_index,
                start,
                len,
                node_count,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedNodeOutOfRange {
                root_key,
                group_index,
                start,
                len,
                node_count,
            },
        }
    }

    pub(super) fn node_count_mismatch(self, expected: usize, actual: usize) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::NodeCountMismatch { expected, actual },
            Self::Detached { root_key } => SlotInvariantError::DetachedNodeCountMismatch {
                root_key,
                expected,
                actual,
            },
        }
    }

    pub(super) fn node_owner_mismatch(
        self,
        node_id: crate::NodeId,
        expected: AnchorId,
        actual: AnchorId,
    ) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::NodeOwnerMismatch {
                node_id,
                expected,
                actual,
            },
            Self::Detached { root_key } => SlotInvariantError::DetachedNodeOwnerMismatch {
                root_key,
                node_id,
                expected,
                actual,
            },
        }
    }

    pub(super) fn duplicate_node_id(self, node_id: crate::NodeId) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::DuplicateNodeId { node_id },
            Self::Detached { root_key } => {
                SlotInvariantError::DetachedDuplicateNodeId { root_key, node_id }
            }
        }
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
            return Err(view
                .kind
                .invalid_parent(index, expected_parent, group.parent_anchor));
        }

        let expected_depth = stack.len() as u32;
        if group.depth != expected_depth {
            return Err(view.kind.bad_depth(index, expected_depth, group.depth));
        }

        let subtree_end = index + group.subtree_len as usize;
        if subtree_end == index || subtree_end > view.groups.len() {
            return Err(view.kind.bad_subtree_len(index, 0, group.subtree_len));
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
        return Err(view
            .kind
            .payload_count_mismatch(expected_payload_start, view.payloads.len()));
    }
    if expected_node_start != view.nodes.len() {
        return Err(view
            .kind
            .node_count_mismatch(expected_node_start, view.nodes.len()));
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
            return Err(view.kind.bad_subtree_len(
                index,
                expected_subtree_len[index],
                group.subtree_len,
            ));
        }
        if group.subtree_node_count != expected_subtree_node_count[index] {
            return Err(view.kind.bad_subtree_node_count(
                index,
                expected_subtree_node_count[index],
                group.subtree_node_count,
            ));
        }
    }

    Ok(())
}
