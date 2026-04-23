use super::GroupRecord;
use crate::{slot_storage::GroupKey, AnchorId, NodeId, ScopeId};
use std::any::{Any, TypeId};

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
    pub(super) value: Box<dyn Any>,
}

#[derive(Clone, Copy)]
pub(super) struct NodeRecord {
    pub(super) owner: AnchorId,
    pub(super) id: NodeId,
    pub(super) generation: u32,
    pub(super) lifecycle: NodeLifecycle,
}

pub(crate) struct DetachedSubtree {
    pub(super) root_key: GroupKey,
    pub(super) root_scope_id: Option<ScopeId>,
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn node_ids(&self) -> Vec<NodeId> {
        self.node_ids_iter().collect()
    }

    pub(crate) fn node_ids_iter(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|node| node.id)
    }

    pub(crate) fn node_states(&self) -> impl Iterator<Item = (NodeId, NodeLifecycle)> + '_ {
        self.nodes.iter().map(|node| (node.id, node.lifecycle))
    }

    pub(crate) fn node_count(&self) -> usize {
        self.groups
            .first()
            .map(|group| group.subtree_node_count as usize)
            .unwrap_or(0)
    }

    pub(crate) fn group_count(&self) -> usize {
        self.groups.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn scope_ids(&self) -> Vec<ScopeId> {
        self.scope_ids_iter().collect()
    }

    pub(crate) fn scope_ids_iter(&self) -> impl Iterator<Item = ScopeId> + '_ {
        self.groups.iter().filter_map(|group| group.scope_id)
    }

    pub(crate) fn scope_count(&self) -> usize {
        self.groups
            .iter()
            .filter(|group| group.scope_id.is_some())
            .count()
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

    pub(crate) fn into_payload_values_rev(self) -> Vec<Box<dyn Any>> {
        self.payloads
            .into_iter()
            .rev()
            .map(|payload| payload.value)
            .collect()
    }
}

pub(crate) struct FinishGroupResult {
    pub(crate) detached_children: Vec<DetachedSubtree>,
    pub(crate) structure_changed: bool,
    pub(crate) direct_nodes: Vec<NodeId>,
}
