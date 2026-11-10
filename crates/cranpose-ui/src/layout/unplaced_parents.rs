//! The parents of the nodes a layout pass left unplaced.

use cranpose_core::{MemoryApplier, NodeId};

use super::{LiveLayoutNode, read_layout_node, read_live_layout_node};
use crate::render_state::AppContext;

// Names to the scene phase the parent of every node `pass` found placed and
// left unplaced: no geometry setter reported those, yet their layers leave
// the scene. A parent is a node that lists the unplaced node among its
// children and that a walk from `root` down those lists comes to, where
// the walk stops at an unplaced node. The pass kept the nodes it cleared,
// so only their parents are read. Out of line: a pass rarely unplaces
// nodes.
#[inline(never)]
pub(super) fn record_unplaced_parents(
    applier: &mut MemoryApplier,
    root: NodeId,
    pass: u64,
    context: &AppContext,
) {
    let cleared = context.placement_pass().take_cleared();
    let mut parents = Vec::new();
    for &node in &cleared {
        let unplaced = read_layout_node(applier, node, |state, _| state.unplaced_in(pass));
        if matches!(unplaced, Ok(Some(true))) {
            parents.extend(listers(applier, node).into_iter().flatten());
        }
    }
    context.placement_pass().keep_cleared(cleared);
    parents.sort_unstable();
    parents.dedup();
    let mut children = Vec::new();
    for parent in parents {
        children.clear();
        let _ = read_layout_node(applier, parent, |_, ids| children.extend_from_slice(ids));
        let lists_unplaced = children.iter().any(|&child| {
            matches!(
                read_layout_node(applier, child, |state, _| state.unplaced_in(pass)),
                Ok(Some(true))
            )
        });
        if lists_unplaced && walk_reaches(applier, root, parent, pass) {
            context.record_geometry_scene_node(parent);
        }
    }
}

// The nodes that may list `node` among their children: its direct parent,
// and the node above that parent, as a subcompose node lists the nodes it
// placed, which lie under its slot hosts.
fn listers(applier: &mut MemoryApplier, node: NodeId) -> [Option<NodeId>; 2] {
    let direct_parent = |applier: &mut MemoryApplier, node| {
        read_live_layout_node(applier, node, |node| node.direct_parent())
            .ok()
            .flatten()
            .flatten()
    };
    let direct = direct_parent(applier, node);
    let above = direct.and_then(|direct| direct_parent(applier, direct));
    [direct, above]
}

// Whether a walk from `root` down the children each node lists, which stops
// at a node `pass` left unplaced, comes to `node`.
fn walk_reaches(applier: &mut MemoryApplier, root: NodeId, node: NodeId, pass: u64) -> bool {
    if node == root {
        return true;
    }
    let unplaced = read_layout_node(applier, node, |state, _| state.unplaced_in(pass));
    if !matches!(unplaced, Ok(Some(false))) {
        return false;
    }
    let [direct, above] = listers(applier, node);
    [direct, above].into_iter().flatten().any(|up| {
        // A layout node lists its direct children, a subcompose node the
        // nodes it placed: the walk comes to such a node from the
        // subcompose node and never to the slot host between them.
        let listed = read_live_layout_node(applier, up, |lister| match lister {
            LiveLayoutNode::Layout(_) => direct == Some(up),
            LiveLayoutNode::Subcompose(_) => {
                lister.with_children(|children| children.contains(&node))
            }
        });
        matches!(listed, Ok(Some(true))) && walk_reaches(applier, root, up, pass)
    })
}
