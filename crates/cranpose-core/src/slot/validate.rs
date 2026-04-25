#[cfg(any(test, debug_assertions))]
use super::{
    AnchorState, DetachedSubtree, GroupRecord, NodeLifecycle, NodeRecord, PayloadRecord, SlotTable,
};
#[cfg(any(test, debug_assertions))]
use crate::collections::map::{HashMap, HashSet};
#[cfg(any(test, debug_assertions))]
use crate::{slot_storage::GroupKey, AnchorId, NodeId, ScopeId};

#[cfg(any(test, debug_assertions))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotInvariantError {
    GroupAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    AnchorMismatch {
        anchor: AnchorId,
        expected: usize,
        actual: Option<AnchorState>,
    },
    InvalidParent {
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    BadDepth {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeLen {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeNodeCount {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    PayloadStartMismatch {
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    PayloadOutOfRange {
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    },
    PayloadCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    ScopeIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadOwnerMismatch {
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    PayloadLocationMismatch {
        payload_anchor: usize,
        expected: (AnchorId, usize),
        actual: Option<(AnchorId, usize)>,
    },
    NodeStartMismatch {
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    NodeOutOfRange {
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    },
    NodeCountMismatch {
        expected: usize,
        actual: usize,
    },
    NodeOwnerMismatch {
        node_id: NodeId,
        expected: AnchorId,
        actual: AnchorId,
    },
    NodeLifecycleMismatch {
        node_id: NodeId,
        expected: NodeLifecycle,
        actual: NodeLifecycle,
    },
    DuplicateSiblingKey {
        parent_anchor: AnchorId,
        key: GroupKey,
    },
    ScopeIndexMismatch {
        scope_id: ScopeId,
        expected: AnchorId,
        actual: Option<AnchorId>,
    },
    RetainedRootKeyMismatch {
        parent_scope: Option<ScopeId>,
        expected: GroupKey,
        actual: GroupKey,
    },
    RetainedSubtreeAnchorStillActive {
        root_key: GroupKey,
        anchor: AnchorId,
        active_index: usize,
    },
    RetainedAnchorStateMismatch {
        root_key: GroupKey,
        anchor: AnchorId,
        actual: Option<AnchorState>,
    },
    RetainedScopeStillActive {
        root_key: GroupKey,
        scope_id: ScopeId,
        active_anchor: AnchorId,
    },
    RetainedRootHasActiveParent {
        root_key: GroupKey,
        parent_anchor: AnchorId,
    },
    RetainedNodeLifecycleMismatch {
        root_key: GroupKey,
        node_id: NodeId,
        actual: NodeLifecycle,
    },
    DetachedSubtreeEmpty,
    DetachedDuplicateAnchor {
        root_key: GroupKey,
        anchor: AnchorId,
    },
    DetachedInvalidParent {
        root_key: GroupKey,
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    DetachedBadDepth {
        root_key: GroupKey,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    DetachedBadSubtreeLen {
        root_key: GroupKey,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    DetachedBadSubtreeNodeCount {
        root_key: GroupKey,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    DetachedPayloadStartMismatch {
        root_key: GroupKey,
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    DetachedPayloadOutOfRange {
        root_key: GroupKey,
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    },
    DetachedPayloadCountMismatch {
        root_key: GroupKey,
        expected: usize,
        actual: usize,
    },
    DetachedPayloadOwnerMismatch {
        root_key: GroupKey,
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    DetachedNodeStartMismatch {
        root_key: GroupKey,
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    DetachedNodeOutOfRange {
        root_key: GroupKey,
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    },
    DetachedNodeCountMismatch {
        root_key: GroupKey,
        expected: usize,
        actual: usize,
    },
    DetachedNodeOwnerMismatch {
        root_key: GroupKey,
        node_id: NodeId,
        expected: AnchorId,
        actual: AnchorId,
    },
    WriterFrameOutOfBounds {
        frame_index: usize,
        group_anchor: AnchorId,
        field: &'static str,
        value: usize,
        min: usize,
        max: usize,
    },
    WriterFrameNotAtChildBoundary {
        frame_index: usize,
        group_anchor: AnchorId,
        next_child_index: usize,
        expected_parent: AnchorId,
        actual_parent: AnchorId,
    },
}

#[cfg(any(test, debug_assertions))]
struct SlotTreeView<'a> {
    kind: SlotTreeKind,
    groups: &'a [GroupRecord],
    payloads: &'a [PayloadRecord],
    nodes: &'a [NodeRecord],
}

#[cfg(any(test, debug_assertions))]
#[derive(Clone, Copy)]
enum SlotTreeKind {
    Active,
    Detached { root_key: GroupKey },
}

#[cfg(any(test, debug_assertions))]
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

    fn payload_start_mismatch(
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

    fn payload_out_of_range(
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

    fn payload_count_mismatch(self, expected: usize, actual: usize) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::PayloadCountMismatch { expected, actual },
            Self::Detached { root_key } => SlotInvariantError::DetachedPayloadCountMismatch {
                root_key,
                expected,
                actual,
            },
        }
    }

    fn payload_owner_mismatch(
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

    fn node_start_mismatch(
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

    fn node_out_of_range(
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

    fn node_count_mismatch(self, expected: usize, actual: usize) -> SlotInvariantError {
        match self {
            Self::Active => SlotInvariantError::NodeCountMismatch { expected, actual },
            Self::Detached { root_key } => SlotInvariantError::DetachedNodeCountMismatch {
                root_key,
                expected,
                actual,
            },
        }
    }

    fn node_owner_mismatch(
        self,
        node_id: NodeId,
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
}

#[cfg(any(test, debug_assertions))]
trait SlotTreeChecks {
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

#[cfg(any(test, debug_assertions))]
struct ActiveSlotTreeChecks<'a> {
    table: &'a SlotTable,
    sibling_keys: HashMap<(AnchorId, GroupKey), usize>,
}

#[cfg(any(test, debug_assertions))]
impl<'a> ActiveSlotTreeChecks<'a> {
    fn new(table: &'a SlotTable) -> Self {
        Self {
            table,
            sibling_keys: HashMap::default(),
        }
    }
}

#[cfg(any(test, debug_assertions))]
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

#[cfg(any(test, debug_assertions))]
struct DetachedSlotTreeChecks;

#[cfg(any(test, debug_assertions))]
impl SlotTreeChecks for DetachedSlotTreeChecks {}

#[cfg(any(test, debug_assertions))]
fn validate_slot_tree(
    view: SlotTreeView<'_>,
    checks: &mut impl SlotTreeChecks,
) -> Result<(), SlotInvariantError> {
    let mut stack: Vec<(AnchorId, usize)> = Vec::new();
    let mut anchor_to_group: HashMap<AnchorId, usize> = HashMap::default();
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

        let payload_start = group.payload_start as usize;
        if payload_start != expected_payload_start {
            return Err(view.kind.payload_start_mismatch(
                index,
                expected_payload_start,
                payload_start,
            ));
        }
        let payload_len = group.payload_len as usize;
        let payload_end = payload_start.saturating_add(payload_len);
        if payload_end > view.payloads.len() {
            return Err(view.kind.payload_out_of_range(
                index,
                payload_start,
                payload_len,
                view.payloads.len(),
            ));
        }
        for (payload_index, payload) in view.payloads[payload_start..payload_end].iter().enumerate()
        {
            if payload.owner != group.anchor {
                return Err(view.kind.payload_owner_mismatch(
                    payload.anchor,
                    group.anchor,
                    payload.owner,
                ));
            }
            checks.validate_payload(index, group, payload_index, payload)?;
        }
        expected_payload_start = payload_end;

        checks.after_payloads(index, group)?;

        let node_start = group.node_start as usize;
        if node_start != expected_node_start {
            return Err(view
                .kind
                .node_start_mismatch(index, expected_node_start, node_start));
        }
        let node_len = group.node_len as usize;
        let node_end = node_start.saturating_add(node_len);
        if node_end > view.nodes.len() {
            return Err(view
                .kind
                .node_out_of_range(index, node_start, node_len, view.nodes.len()));
        }
        for node in &view.nodes[node_start..node_end] {
            if node.owner != group.anchor {
                return Err(view
                    .kind
                    .node_owner_mismatch(node.id, group.anchor, node.owner));
            }
            checks.validate_node(index, group, node)?;
        }
        expected_node_start = node_end;

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

#[cfg(any(test, debug_assertions))]
impl SlotTable {
    pub(crate) fn debug_verify(&self) {
        if crate::slot_validation_diagnostics_enabled() {
            if let Err(err) = self.validate() {
                panic!("slot table invariant violation: {err:?}");
            }
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

#[cfg(any(test, debug_assertions))]
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
