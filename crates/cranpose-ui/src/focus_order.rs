use cranpose_core::NodeId;
use cranpose_foundation::{SemanticsConfiguration, SemanticsWidgetRole};
use cranpose_ui_graphics::Rect;

use crate::layout::{LayoutBox, LayoutTree};

/// One focus target, with the bounds layout gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusEntry {
    pub node_id: NodeId,
    pub rect: Rect,
}

impl FocusEntry {
    pub fn center(&self) -> (f32, f32) {
        (
            self.rect.x + self.rect.width * 0.5,
            self.rect.y + self.rect.height * 0.5,
        )
    }
}

/// Replaces the focus order a later [`crate::FocusManager`] move reads. The
/// order belongs to the current [`crate::AppContext`]. Keyboard navigation
/// publishes the order from the latest layout before choosing a target.
pub fn set_focus_order(entries: Vec<FocusEntry>) {
    crate::render_state::with_focus_dispatch(|state| state.set_focus_order(entries));
}

/// Reads the published focus order.
pub fn with_focus_order<T>(reader: impl FnOnce(&[FocusEntry]) -> T) -> T {
    crate::render_state::with_focus_dispatch(|state| state.with_focus_order(reader))
}

/// How many focus targets the last published order holds.
pub fn focus_order_len() -> usize {
    with_focus_order(<[FocusEntry]>::len)
}

/// Walks `tree` in the order the layout pass placed it and keeps the nodes
/// that registered a focus target and take space on screen.
pub fn collect_focus_order(tree: &LayoutTree) -> Vec<FocusEntry> {
    let mut entries = Vec::new();
    collect_from_box(focus_root(tree.root()), &mut entries);
    entries
}

fn collect_from_box(layout_box: &LayoutBox, entries: &mut Vec<FocusEntry>) {
    let config = layout_box.node_data.semantics();
    if config.is_some_and(|config| config.hidden) {
        return;
    }
    if crate::focus_dispatch::has_focus_target(layout_box.node_id)
        && takes_space(layout_box.rect)
        && config.is_none_or(|config| config.enabled)
    {
        entries.push(FocusEntry {
            node_id: layout_box.node_id,
            rect: layout_box.rect,
        });
    }
    for child in &layout_box.children {
        collect_from_box(child, entries);
    }
}

pub(crate) fn takes_space(rect: Rect) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
}

/// The focus targets under one node, in the order layout gave them. An arrow
/// key inside a selectable group moves among these and no others.
pub fn collect_focus_order_under(tree: &LayoutTree, node_id: NodeId) -> Vec<FocusEntry> {
    let mut entries = Vec::new();
    if let Some(layout_box) = find_box(focus_root(tree.root()), node_id) {
        collect_from_box(layout_box, &mut entries);
    }
    entries
}

/// The nearest node above the given one that declares
/// [`selectable_group`](crate::Modifier::selectable_group), when there is one.
pub fn selectable_group_of(tree: &LayoutTree, node_id: NodeId) -> Option<NodeId> {
    group_above(focus_root(tree.root()), node_id, None)
}

pub(crate) fn focus_root(root: &LayoutBox) -> &LayoutBox {
    top_modal(root).unwrap_or(root)
}

fn top_modal(layout_box: &LayoutBox) -> Option<&LayoutBox> {
    let config = layout_box.node_data.semantics();
    if config.is_some_and(|config| config.hidden) {
        return None;
    }
    layout_box
        .children
        .iter()
        .rev()
        .find_map(top_modal)
        .or_else(|| {
            (takes_space(layout_box.rect) && config.is_some_and(|config| config.is_modal))
                .then_some(layout_box)
        })
}

fn group_above(layout_box: &LayoutBox, node_id: NodeId, group: Option<NodeId>) -> Option<NodeId> {
    let group = if declares_selectable_group(layout_box) {
        Some(layout_box.node_id)
    } else {
        group
    };
    if layout_box.node_id == node_id {
        return group;
    }
    layout_box
        .children
        .iter()
        .find_map(|child| group_above(child, node_id, group))
}

fn declares_selectable_group(layout_box: &LayoutBox) -> bool {
    layout_box
        .node_data
        .semantics()
        .is_some_and(is_selectable_group)
}

pub(crate) fn is_selectable_group(config: &SemanticsConfiguration) -> bool {
    config.selectable_group
        || matches!(
            config.role,
            Some(
                SemanticsWidgetRole::Menu
                    | SemanticsWidgetRole::RadioGroup
                    | SemanticsWidgetRole::TabBar
            )
        )
}

fn find_box(layout_box: &LayoutBox, node_id: NodeId) -> Option<&LayoutBox> {
    if layout_box.node_id == node_id {
        return Some(layout_box);
    }
    layout_box
        .children
        .iter()
        .find_map(|child| find_box(child, node_id))
}
