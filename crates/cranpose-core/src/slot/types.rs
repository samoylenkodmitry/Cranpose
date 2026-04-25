use super::GroupRecord;
use crate::collections::map::HashSet;
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

pub(crate) struct DetachedSubtree {
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) generation: u64,
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

    pub(crate) fn root_nodes(&self) -> Vec<NodeId> {
        root_node_ids_from_records(&self.nodes)
    }

    pub(crate) fn root_nodes_iter(&self) -> std::vec::IntoIter<NodeId> {
        self.root_nodes().into_iter()
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
    pub(crate) direct_nodes: Vec<NodeId>,
    pub(crate) root_nodes: Vec<NodeId>,
    pub(crate) was_skipped: bool,
}

impl PayloadRecord {
    pub(crate) fn into_deferred_drop(self) -> DeferredDrop {
        DeferredDrop::payload(self.kind, self.value)
    }
}

pub(in crate::slot) fn root_node_ids_from_records(nodes: &[NodeRecord]) -> Vec<NodeId> {
    let node_set = nodes.iter().map(|node| node.id).collect::<HashSet<_>>();
    nodes
        .iter()
        .filter(|node| {
            node.parent_id
                .is_none_or(|parent_id| !node_set.contains(&parent_id))
        })
        .map(|node| node.id)
        .collect()
}
