use super::super::{AnchorState, NodeLifecycle};
use crate::{slot_storage::GroupKey, AnchorId, NodeId, ScopeId};

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
    DuplicateNodeId {
        node_id: NodeId,
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
    DetachedDuplicateNodeId {
        root_key: GroupKey,
        node_id: NodeId,
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
