use cranpose_core::{Applier, MemoryApplier, NodeError, NodeId};

use super::{LayoutNode, read_live_layout_node};

/// Proves that revealing `target` cannot scroll any ancestor in `root`'s surface.
///
/// This follows placed parent links without building snapshots. It returns
/// `true` only after reaching `root` without finding an ancestor scroll action.
/// Missing nodes, unsupported parent paths, inactive children, other windows and
/// scroll actions return `false`; callers must use their usual reveal handling.
/// Modal eligibility and geometry are left to that handling.
pub fn can_skip_scroll_reveal_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    target: NodeId,
) -> Result<bool, NodeError> {
    let mut current = target;
    let mut child = None;
    for _ in 0..applier.capacity() {
        let Some((parent, accepted)) = read_live_layout_node(applier, current, |node| {
            let accepted = node.state().is_placed()
                && (current == root || !node.is_window_root())
                && child.is_none_or(|child| node.with_children(|ids| ids.contains(&child)))
                && (child.is_none()
                    || !node
                        .semantics()
                        .is_some_and(|config| config.scroll_by.is_some()));
            (node.parent(), accepted)
        })?
        else {
            return Ok(false);
        };
        if !accepted {
            return Ok(false);
        }
        if current == root {
            return Ok(true);
        }
        let Some(parent) = parent else {
            return Ok(false);
        };
        let parent = match applier.get_mut(parent) {
            Ok(node) => node
                .as_any_mut()
                .downcast_mut::<LayoutNode>()
                .filter(|node| node.is_virtual())
                .map_or(Some(parent), |node| node.parent()),
            Err(NodeError::Missing { .. }) => None,
            Err(error) => return Err(error),
        };
        let Some(parent) = parent else {
            return Ok(false);
        };
        child = Some(current);
        current = parent;
    }
    Ok(false)
}
