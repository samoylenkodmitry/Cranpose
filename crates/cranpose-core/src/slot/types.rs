use super::{checked_usize_to_u32, DeferredDrop, GroupRecord};
use crate::collections::map::HashSet;
use crate::{AnchorId, Key, NodeId, ScopeId};
use std::any::{Any, TypeId};
use std::mem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SlotPassMode {
    Compose,
    Recompose,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeLifecycle {
    Active,
    RetainedDetached,
}

/// Stable structural identity for a group among siblings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey {
    pub(crate) static_key: Key,
    pub(crate) explicit_key: Option<Key>,
    pub(crate) ordinal: u32,
}

impl GroupKey {
    pub(crate) fn new(static_key: Key, explicit_key: Option<Key>, ordinal: u32) -> Self {
        Self {
            static_key,
            explicit_key,
            ordinal,
        }
    }
}

/// Seed used to reserve a full [`GroupKey`] in the active writer frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKeySeed {
    pub(crate) static_key: Key,
    pub(crate) explicit_key: Option<Key>,
}

impl GroupKeySeed {
    pub(crate) fn unkeyed(static_key: Key) -> Self {
        Self {
            static_key,
            explicit_key: None,
        }
    }

    pub(crate) fn keyed(static_key: Key, explicit_key: Key) -> Self {
        Self {
            static_key,
            explicit_key: Some(explicit_key),
        }
    }
}

/// Transient handle to a group in the active slot table.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ActiveGroupId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl ActiveGroupId {
    pub(crate) fn new(index: usize, generation: u32) -> Self {
        Self {
            index: checked_usize_to_u32(index, "active group id index"),
            generation,
        }
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

/// Stable semantic identity for a payload record.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct PayloadAnchor {
    id: u32,
    generation: u32,
}

impl PayloadAnchor {
    pub(crate) fn new(id: usize, generation: u32) -> Self {
        Self {
            id: checked_usize_to_u32(id, "payload anchor id"),
            generation,
        }
    }

    pub(crate) fn id(self) -> usize {
        self.id as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }

    pub(crate) fn with_generation(self, generation: u32) -> Self {
        Self {
            id: self.id,
            generation,
        }
    }
}

/// Opaque handle to a value slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ValueSlotId {
    pub(crate) anchor: PayloadAnchor,
    storage_id: usize,
}

impl ValueSlotId {
    pub(crate) fn new_for_table(anchor: PayloadAnchor, storage_id: usize) -> Self {
        Self { anchor, storage_id }
    }

    pub(crate) fn anchor(self) -> PayloadAnchor {
        self.anchor
    }

    pub(crate) fn storage_id(self) -> usize {
        self.storage_id
    }
}

/// Semantic result of starting a group at the current writer cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupStartKind {
    Inserted,
    Reused,
    Moved,
    Restored,
}

/// Semantic input required to begin a group at the current writer cursor.
pub(crate) struct BeginGroupInput<R> {
    pub(crate) key: GroupKey,
    pub(crate) restored: Option<R>,
}

impl<R> BeginGroupInput<R> {
    pub(crate) fn new(key: GroupKey, restored: Option<R>) -> Self {
        Self { key, restored }
    }
}

/// Result of starting a group.
pub(crate) struct GroupStart<G> {
    pub(crate) group: G,
    pub(crate) anchor: AnchorId,
    pub(crate) scope_id: Option<ScopeId>,
    pub(crate) kind: GroupStartKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeSlotUpdate {
    Reused {
        id: NodeId,
        generation: u32,
    },
    Inserted {
        id: NodeId,
        generation: u32,
    },
    Replaced {
        old_id: NodeId,
        old_generation: u32,
        new_id: NodeId,
        new_generation: u32,
    },
}

pub(super) struct PayloadRecord {
    pub(super) owner: AnchorId,
    pub(super) anchor: PayloadAnchor,
    pub(super) type_id: TypeId,
    pub(super) type_name: &'static str,
    pub(super) kind: PayloadKind,
    pub(super) value: Box<dyn Any>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PayloadKind {
    Remember,
    Param,
    Return,
    Effect,
    Internal,
}

impl PayloadKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Param => "param",
            Self::Return => "return",
            Self::Effect => "effect",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct NodeRecord {
    pub(super) owner: AnchorId,
    pub(super) id: NodeId,
    pub(super) parent_id: Option<NodeId>,
    pub(super) generation: u32,
    pub(super) lifecycle: NodeLifecycle,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChildCursor {
    parent: AnchorId,
    index: usize,
}

impl ChildCursor {
    pub(crate) fn new(parent: AnchorId, index: usize) -> Self {
        Self { parent, index }
    }

    pub(crate) fn parent(self) -> AnchorId {
        self.parent
    }

    pub(crate) fn index(self) -> usize {
        self.index
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActiveSubtreeRoot {
    anchor: AnchorId,
}

impl ActiveSubtreeRoot {
    pub(crate) fn new(anchor: AnchorId) -> Self {
        Self { anchor }
    }

    pub(crate) fn anchor(self) -> AnchorId {
        self.anchor
    }
}

pub(crate) struct DetachedSubtree {
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
}

pub(crate) struct DetachedChild {
    expected_key: GroupKey,
    subtree: DetachedSubtree,
}

impl DetachedChild {
    pub(crate) fn new(expected_key: GroupKey, subtree: DetachedSubtree) -> Self {
        Self {
            expected_key,
            subtree,
        }
    }

    pub(crate) fn expected_key(&self) -> GroupKey {
        self.expected_key
    }

    pub(crate) fn into_subtree(self) -> DetachedSubtree {
        self.subtree
    }
}

impl DetachedSubtree {
    fn root(&self) -> &GroupRecord {
        self.groups
            .first()
            .expect("detached subtree must contain a root group")
    }

    pub(crate) fn root_key(&self) -> GroupKey {
        self.root().key
    }

    pub(crate) fn root_parent_anchor(&self) -> AnchorId {
        self.root().parent_anchor
    }

    pub(crate) fn root_scope_id(&self) -> Option<ScopeId> {
        self.root().scope_id
    }

    pub(crate) fn node_ids_iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|node| node.id)
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn node_states(&self) -> impl Iterator<Item = (NodeId, NodeLifecycle)> + '_ {
        self.nodes.iter().map(|node| (node.id, node.lifecycle))
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn payload_count(&self) -> usize {
        self.payloads.len()
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn payload_anchors(&self) -> impl Iterator<Item = PayloadAnchor> + '_ {
        self.payloads.iter().map(|payload| payload.anchor)
    }

    pub(crate) fn collect_root_nodes_into(&self, root_nodes: &mut Vec<NodeId>) {
        collect_root_node_ids_from_records_into(&self.nodes, root_nodes);
    }

    pub(crate) fn group_count(&self) -> usize {
        self.groups.len()
    }

    pub(crate) fn scope_ids(&self) -> Vec<ScopeId> {
        self.scope_ids_iter().collect()
    }

    pub(crate) fn scope_ids_iter(&self) -> impl Iterator<Item = ScopeId> + '_ {
        self.groups.iter().filter_map(|group| group.scope_id)
    }

    pub(crate) fn scope_count(&self) -> usize {
        self.scope_ids_iter().count()
    }

    pub(crate) fn anchor_count(&self) -> usize {
        self.groups.len()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.groups.capacity() * mem::size_of::<GroupRecord>()
            + self.payloads.capacity() * mem::size_of::<PayloadRecord>()
            + self.nodes.capacity() * mem::size_of::<NodeRecord>()
    }

    pub(crate) fn group_anchors(&self) -> impl Iterator<Item = AnchorId> + '_ {
        self.groups.iter().map(|group| group.anchor)
    }

    pub(crate) fn mark_nodes_retained_detached(&mut self) {
        self.set_node_lifecycle(NodeLifecycle::RetainedDetached);
    }

    pub(crate) fn mark_nodes_active(&mut self) {
        self.set_node_lifecycle(NodeLifecycle::Active);
    }

    pub(crate) fn assert_root_key(&self, expected: GroupKey, context: &'static str) {
        assert_eq!(
            self.root_key(),
            expected,
            "{context}: detached subtree root key mismatch"
        );
    }

    pub(crate) fn assert_root_parent_detached(&self, context: &'static str) {
        assert!(
            !self.root_parent_anchor().is_valid(),
            "{context}: detached subtree root parent must be detached"
        );
    }

    pub(crate) fn assert_node_lifecycle(&self, expected: NodeLifecycle, context: &'static str) {
        for node in &self.nodes {
            assert_eq!(
                node.lifecycle, expected,
                "{context}: detached subtree node lifecycle mismatch"
            );
        }
    }

    fn set_node_lifecycle(&mut self, lifecycle: NodeLifecycle) {
        for node in &mut self.nodes {
            node.lifecycle = lifecycle;
        }
    }

    pub(crate) fn into_payload_drops_rev(self) -> Vec<DeferredDrop> {
        self.payloads
            .into_iter()
            .rev()
            .map(PayloadRecord::into_deferred_drop)
            .collect()
    }
}

pub(crate) struct FinishGroupResult {
    pub(crate) detached_children: Vec<DetachedSubtree>,
    pub(crate) direct_nodes: Vec<NodeId>,
    pub(crate) root_nodes: Vec<NodeId>,
    pub(crate) was_skipped: bool,
}

impl PayloadRecord {
    pub(crate) fn into_deferred_drop(self) -> DeferredDrop {
        DeferredDrop::payload(self.value)
    }
}

pub(in crate::slot) fn collect_root_node_ids_from_records_into(
    nodes: &[NodeRecord],
    root_nodes: &mut Vec<NodeId>,
) {
    root_nodes.clear();
    root_nodes.reserve(nodes.len());

    let mut node_set: HashSet<NodeId> = HashSet::default();
    node_set.reserve(nodes.len());
    node_set.extend(nodes.iter().map(|node| node.id));

    root_nodes.extend(nodes.iter().filter_map(|node| {
        node.parent_id
            .is_none_or(|parent_id| !node_set.contains(&parent_id))
            .then_some(node.id)
    }));
}
