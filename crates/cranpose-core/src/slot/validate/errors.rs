use super::super::{AnchorState, NodeLifecycle, PayloadAnchorLifecycle};
use crate::{
    slot_storage::{GroupKey, PayloadAnchor},
    AnchorId, NodeId, ScopeId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotTreeContext {
    Active,
    Detached { root_key: GroupKey },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadLocationRecord {
    pub(crate) owner: AnchorId,
    pub(crate) payload_anchor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PayloadAnchorRecord {
    pub(crate) owner: AnchorId,
    pub(crate) payload_anchor: PayloadAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotInvariantError {
    GroupAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    AnchorRegistryInternalMismatch {
        detail: &'static str,
        anchor_id: Option<u32>,
        expected: usize,
        actual: usize,
    },
    AnchorMismatch {
        anchor: AnchorId,
        expected: usize,
        actual: Option<AnchorState>,
    },
    AnchorTargetMismatch {
        anchor: AnchorId,
        expected_group_index: usize,
        actual: Option<AnchorId>,
    },
    InvalidParent {
        tree: SlotTreeContext,
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    BadDepth {
        tree: SlotTreeContext,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeLen {
        tree: SlotTreeContext,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeNodeCount {
        tree: SlotTreeContext,
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    PayloadStartMismatch {
        tree: SlotTreeContext,
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    PayloadOutOfRange {
        tree: SlotTreeContext,
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    },
    PayloadCountMismatch {
        tree: SlotTreeContext,
        expected: usize,
        actual: usize,
    },
    PayloadAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadAnchorRegistryCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadAnchorRegistryInternalMismatch {
        detail: &'static str,
        payload_anchor_id: Option<usize>,
        expected: usize,
        actual: usize,
    },
    ScopeIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadOwnerMismatch {
        tree: SlotTreeContext,
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    DuplicatePayloadAnchor {
        tree: SlotTreeContext,
        payload_anchor: PayloadAnchor,
    },
    PayloadLocationTargetMismatch {
        payload_anchor: usize,
        expected_owner: AnchorId,
        expected_payload_index: usize,
        actual: Option<PayloadLocationRecord>,
    },
    PayloadAnchorRegistryMismatch {
        payload_anchor: PayloadAnchor,
        expected: (AnchorId, usize),
        actual: Option<(AnchorId, usize)>,
    },
    PayloadAnchorRegistryTargetMismatch {
        payload_anchor: PayloadAnchor,
        expected_owner: AnchorId,
        expected_payload_index: usize,
        actual: Option<PayloadAnchorRecord>,
    },
    NodeStartMismatch {
        tree: SlotTreeContext,
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    NodeOutOfRange {
        tree: SlotTreeContext,
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    },
    NodeCountMismatch {
        tree: SlotTreeContext,
        expected: usize,
        actual: usize,
    },
    NodeOwnerMismatch {
        tree: SlotTreeContext,
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
        tree: SlotTreeContext,
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
    RetainedPayloadAnchorStillActive {
        root_key: GroupKey,
        payload_anchor: PayloadAnchor,
        active_owner: AnchorId,
        active_index: usize,
    },
    RetainedPayloadAnchorStateMismatch {
        root_key: GroupKey,
        payload_anchor: PayloadAnchor,
        actual: Option<PayloadAnchorLifecycle>,
    },
    DetachedSubtreeEmpty,
    DetachedDuplicateAnchor {
        root_key: GroupKey,
        anchor: AnchorId,
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
