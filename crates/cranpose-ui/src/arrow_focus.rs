use cranpose_core::NodeId;

use crate::{
    FocusDirection, LayoutTree, focus_dispatch,
    focus_manager::arrow_target,
    focus_order::{self, FocusEntry},
    layout::LayoutBox,
    two_dimensional_focus_search::Arrow,
};

/// The target an arrow key moves focus to, by Compose's two-dimensional
/// focus search: inside the focused target's innermost focus group first,
/// then in each group around it, then on the whole screen. A move that
/// enters a [`focus_restorer`](crate::Modifier::focus_restorer) group lands
/// on the child that group last held, or on its fallback, and a move that
/// leaves one makes the group remember the target focus left. Publishes the
/// screen's focus order for later [`crate::FocusManager`] moves.
///
/// [`focus_group`](crate::Modifier::focus_group) and
/// [`selectable_group`](crate::Modifier::selectable_group) nodes are groups.
pub fn arrow_focus_target(tree: &LayoutTree, direction: FocusDirection) -> Option<NodeId> {
    let arrow = Arrow::of(direction)?;
    let root = focus_order::focus_root(tree.root());
    let active = focus_dispatch::active_focus_target();
    let around_focus = active.map_or_else(Vec::new, |focused| groups_above(root, focused));
    let order = focus_order::collect_focus_order(tree);
    let mut target = around_focus
        .iter()
        .find_map(|&group| {
            arrow_target(
                &focus_order::collect_focus_order_under(tree, group),
                active,
                arrow,
            )
        })
        .or_else(|| arrow_target(&order, active, arrow))?;

    let around_target = groups_above(root, target);
    if let Some(restored) = around_target
        .iter()
        .rev()
        .filter(|group| !around_focus.contains(group))
        .find_map(|&group| restored_child(root, group, &order))
    {
        target = restored;
    }
    if let Some(focused) = active {
        for &group in around_focus
            .iter()
            .filter(|group| !around_target.contains(group))
        {
            if let Some(restorer) = focus_dispatch::focus_restorer(group) {
                restorer.save(focused);
            }
        }
    }
    focus_order::set_focus_order(order);
    focus_dispatch::note_focus_rect(target);
    Some(target)
}

// The child a restoring `group` gives focus back to, when it is still a
// target on screen inside the group.
fn restored_child(root: &LayoutBox, group: NodeId, order: &[FocusEntry]) -> Option<NodeId> {
    let restorer = focus_dispatch::focus_restorer(group)?;
    let group_box = focus_order::find_box(root, group)?;
    restorer.restored().find(|&child| {
        order.iter().any(|entry| entry.node_id == child)
            && focus_order::find_box(group_box, child).is_some()
    })
}

// The focus groups around `node_id`, innermost first.
fn groups_above(root: &LayoutBox, node_id: NodeId) -> Vec<NodeId> {
    let mut groups = Vec::new();
    collect_groups_above(root, node_id, &mut groups);
    groups
}

fn collect_groups_above(layout_box: &LayoutBox, node_id: NodeId, groups: &mut Vec<NodeId>) -> bool {
    if layout_box.node_id == node_id {
        return true;
    }
    let found = layout_box
        .children
        .iter()
        .any(|child| collect_groups_above(child, node_id, groups));
    if found && is_group(layout_box) {
        groups.push(layout_box.node_id);
    }
    found
}

fn is_group(layout_box: &LayoutBox) -> bool {
    focus_dispatch::is_focus_group(layout_box.node_id)
        || layout_box
            .node_data
            .semantics()
            .is_some_and(focus_order::is_selectable_group)
}
