use super::{
    super::{GroupRecord, NodeLifecycle, NodeRecord},
    groups::{SlotTreeChecks, SlotTreeView},
    SlotInvariantError,
};
use crate::{collections::map::HashSet, NodeId};

pub(super) fn validate_group_nodes(
    view: &SlotTreeView<'_>,
    checks: &mut impl SlotTreeChecks,
    node_ids: &mut HashSet<NodeId>,
    group_index: usize,
    group: &GroupRecord,
    expected_node_start: usize,
) -> Result<usize, SlotInvariantError> {
    let node_start = group.node_start as usize;
    if node_start != expected_node_start {
        return Err(view.node_start_mismatch(group_index, expected_node_start, node_start));
    }

    let node_len = group.node_len as usize;
    let node_end = node_start.saturating_add(node_len);
    if node_end > view.nodes.len() {
        return Err(view.node_out_of_range(group_index, node_start, node_len, view.nodes.len()));
    }

    for node in &view.nodes[node_start..node_end] {
        if !node_ids.insert(node.id) {
            return Err(view.duplicate_node_id(node.id));
        }
        if node.owner != group.anchor {
            return Err(view.node_owner_mismatch(node.id, group.anchor, node.owner));
        }
        checks.validate_node(group_index, group, node)?;
    }

    Ok(node_end)
}

pub(super) fn validate_active_node_lifecycle(node: &NodeRecord) -> Result<(), SlotInvariantError> {
    if node.lifecycle == NodeLifecycle::Active {
        return Ok(());
    }

    Err(SlotInvariantError::NodeLifecycleMismatch {
        node_id: node.id,
        expected: NodeLifecycle::Active,
        actual: node.lifecycle,
    })
}
