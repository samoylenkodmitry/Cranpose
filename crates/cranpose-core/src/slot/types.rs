use super::GroupRecord;
use crate::slot::DeferredDrop;
use crate::{slot_storage::GroupKey, AnchorId, NodeId, ScopeId};
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
    Disposed,
}

pub(super) struct PayloadRecord {
    pub(super) owner: AnchorId,
    pub(super) anchor: usize,
    pub(super) generation: u32,
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
    Scope,
    Internal,
}

impl PayloadKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Param => "param",
            Self::Return => "return",
            Self::Effect => "effect",
            Self::Scope => "scope",
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

pub(crate) struct DetachedAnchorSet {
    pub(super) group_anchors: Vec<AnchorId>,
}

pub(crate) struct DetachedSubtree {
    pub(super) root_key: GroupKey,
    pub(super) root_scope_id: Option<ScopeId>,
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) root_nodes: Vec<NodeId>,
    pub(super) scope_ids: Vec<ScopeId>,
    pub(super) anchors: DetachedAnchorSet,
    pub(super) generation: u64,
}

impl DetachedSubtree {
    pub(crate) fn root_key(&self) -> GroupKey {
        self.root_key
    }

    pub(crate) fn root_parent_anchor(&self) -> AnchorId {
        self.groups
            .first()
            .map(|group| group.parent_anchor)
            .unwrap_or(AnchorId::INVALID)
    }

    pub(crate) fn root_scope_id(&self) -> Option<ScopeId> {
        self.root_scope_id
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn node_ids_iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|node| node.id)
    }

    pub(crate) fn node_states(&self) -> impl Iterator<Item = (NodeId, NodeLifecycle)> + '_ {
        self.nodes.iter().map(|node| (node.id, node.lifecycle))
    }

    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn payload_count(&self) -> usize {
        self.payloads.len()
    }

    pub(crate) fn root_nodes(&self) -> &[NodeId] {
        &self.root_nodes
    }

    pub(crate) fn root_nodes_iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.root_nodes.iter().copied()
    }

    pub(crate) fn group_count(&self) -> usize {
        self.groups.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn scope_ids(&self) -> Vec<ScopeId> {
        self.scope_ids.clone()
    }

    pub(crate) fn scope_ids_iter(&self) -> impl Iterator<Item = ScopeId> + '_ {
        self.scope_ids.iter().copied()
    }

    pub(crate) fn scope_count(&self) -> usize {
        self.scope_ids.len()
    }

    pub(crate) fn anchor_count(&self) -> usize {
        self.anchors.group_anchors.len()
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.groups.capacity() * mem::size_of::<GroupRecord>()
            + self.payloads.capacity() * mem::size_of::<PayloadRecord>()
            + self.nodes.capacity() * mem::size_of::<NodeRecord>()
            + self.root_nodes.capacity() * mem::size_of::<NodeId>()
            + self.scope_ids.capacity() * mem::size_of::<ScopeId>()
            + self.anchors.group_anchors.capacity() * mem::size_of::<AnchorId>()
    }

    pub(crate) fn group_anchors(&self) -> impl Iterator<Item = AnchorId> + '_ {
        self.anchors.group_anchors.iter().copied()
    }

    pub(crate) fn mark_nodes_retained_detached(&mut self) {
        self.set_node_lifecycle(NodeLifecycle::RetainedDetached);
    }

    pub(crate) fn mark_nodes_disposed(&mut self) {
        self.set_node_lifecycle(NodeLifecycle::Disposed);
    }

    pub(crate) fn mark_nodes_active(&mut self) {
        self.set_node_lifecycle(NodeLifecycle::Active);
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
    pub(crate) structure_changed: bool,
    pub(crate) direct_nodes: Vec<NodeId>,
    pub(crate) subtree_nodes: Vec<NodeId>,
    pub(crate) root_nodes: Vec<NodeId>,
    pub(crate) was_skipped: bool,
}

impl PayloadRecord {
    pub(crate) fn into_deferred_drop(self) -> DeferredDrop {
        DeferredDrop::payload(self.kind, self.value)
    }
}
