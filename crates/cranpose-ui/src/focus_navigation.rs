use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_foundation::SemanticsWidgetRole;

use crate::{KeyCode, LayoutTree, focus_dispatch, focus_order::FocusEntry};

/// A keyboard navigation destination and whether moving there selects it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardFocusTarget {
    /// The enabled, visible focus target to receive focus.
    pub node_id: NodeId,
    /// Whether navigation should activate this target, as radio navigation does.
    pub activate: bool,
}

struct Target {
    entry: FocusEntry,
    group: Option<NodeId>,
    role: Option<SemanticsWidgetRole>,
    selected: bool,
}

fn targets(tree: &LayoutTree) -> Vec<Target> {
    let mut output = Vec::new();
    collect_targets(
        crate::focus_order::focus_root(tree.root()),
        None,
        &mut output,
    );
    output
}

fn collect_targets(node: &crate::LayoutBox, group: Option<NodeId>, output: &mut Vec<Target>) {
    let config = node.node_data.semantics();
    if config.is_some_and(|config| config.hidden) {
        return;
    }
    if focus_dispatch::has_focus_target(node.node_id)
        && crate::focus_order::takes_space(node.rect)
        && config.is_none_or(|config| config.enabled)
    {
        output.push(Target {
            entry: FocusEntry {
                node_id: node.node_id,
                rect: node.rect,
            },
            group,
            role: config.and_then(|config| config.role),
            selected: config.is_some_and(|config| config.selected == Some(true)),
        });
    }
    let group = if config.is_some_and(crate::focus_order::is_selectable_group) {
        Some(node.node_id)
    } else {
        group
    };
    for child in &node.children {
        collect_targets(child, group, output);
    }
}

fn is_composite_member(target: &Target) -> bool {
    target.group.is_some()
        && matches!(
            target.role,
            Some(
                SemanticsWidgetRole::RadioButton
                    | SemanticsWidgetRole::Tab
                    | SemanticsWidgetRole::MenuItem
            )
        )
}

fn tab_targets(targets: &[Target], current: Option<NodeId>) -> Vec<&Target> {
    let mut stops: HashMap<NodeId, &Target> = HashMap::default();
    for target in targets.iter().filter(|target| is_composite_member(target)) {
        let group = target.group.expect("composite member has a group");
        let stop = stops.entry(group).or_insert(target);
        if Some(target.entry.node_id) == current
            || (Some(stop.entry.node_id) != current && target.selected && !stop.selected)
        {
            *stop = target;
        }
    }
    targets
        .iter()
        .filter(|target| {
            !is_composite_member(target)
                || target
                    .group
                    .is_some_and(|group| stops[&group].entry.node_id == target.entry.node_id)
        })
        .collect()
}

/// Resolves Tab, arrows, Home and End using desktop keyboard conventions.
/// Tab enters a radio group or tab list at its focused or selected member and
/// leaves it in one step. Arrows wrap within the group; radio navigation also
/// selects the destination. Tabs use manual activation with Enter or Space.
/// Hidden, disabled and background targets outside the top modal are excluded.
/// Publishes this layout's focus order in the current [`crate::AppContext`]
/// for subsequent programmatic [`crate::FocusManager`] moves.
/// Unhandled keys return `None` so a widget can apply its own keyboard behavior.
pub fn keyboard_focus_target(
    tree: &LayoutTree,
    current: Option<NodeId>,
    key: KeyCode,
    shift: bool,
) -> Option<KeyboardFocusTarget> {
    let targets = targets(tree);
    crate::set_focus_order(targets.iter().map(|target| target.entry).collect());
    if key == KeyCode::Tab {
        let stops = tab_targets(&targets, current);
        let from = stops
            .iter()
            .position(|target| Some(target.entry.node_id) == current);
        let index = stepped_index(stops.len(), from, !shift)?;
        return Some(KeyboardFocusTarget {
            node_id: stops[index].entry.node_id,
            activate: false,
        });
    }
    let current = targets
        .iter()
        .find(|target| Some(target.entry.node_id) == current)?;
    if !is_composite_member(current) {
        return None;
    }
    let members: Vec<_> = targets
        .iter()
        .filter(|target| target.group == current.group && target.role == current.role)
        .collect();
    let from = members
        .iter()
        .position(|target| target.entry.node_id == current.entry.node_id)?;
    let radio = current.role == Some(SemanticsWidgetRole::RadioButton);
    let index = group_key_index(&members, from, key, radio)?;
    Some(KeyboardFocusTarget {
        node_id: members[index].entry.node_id,
        activate: radio,
    })
}

fn group_key_index(members: &[&Target], from: usize, key: KeyCode, radio: bool) -> Option<usize> {
    let horizontal = members.get(1).is_some_and(|second| {
        let first = members[0].entry.rect;
        (second.entry.rect.x - first.x).abs() >= (second.entry.rect.y - first.y).abs()
    });
    Some(match key {
        KeyCode::Home => 0,
        KeyCode::End => members.len() - 1,
        KeyCode::ArrowLeft | KeyCode::ArrowRight if radio || horizontal => {
            stepped_index(members.len(), Some(from), key == KeyCode::ArrowRight)?
        }
        KeyCode::ArrowUp | KeyCode::ArrowDown if radio || !horizontal => {
            stepped_index(members.len(), Some(from), key == KeyCode::ArrowDown)?
        }
        _ => return None,
    })
}

fn stepped_index(count: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, forward) {
        (Some(index), true) => (index + 1) % count,
        (Some(index), false) => (index + count - 1) % count,
        (None, true) => 0,
        (None, false) => count - 1,
    })
}

#[cfg(test)]
#[path = "tests/focus_navigation_tests.rs"]
mod tests;
