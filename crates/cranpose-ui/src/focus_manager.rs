use std::cell::RefCell;

use cranpose_core::{CompositionLocal, NodeId};

use crate::{
    focus_dispatch,
    focus_order::{FocusEntry, with_focus_order},
    modifier::FocusDirection,
};

/// Moves focus between the focus targets the tree holds, the way
/// `LocalFocusManager` does in Jetpack Compose.
///
/// ```ignore
/// let focus = cranpose_ui::local_focus_manager().current();
/// focus.move_focus(FocusDirection::Next);
/// focus.clear_focus();
/// ```
///
/// Tab and Shift+Tab reach this through the app shell, so an app that puts
/// `Modifier::focusable()` on its controls gets keyboard traversal with no
/// further code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusManager;

impl FocusManager {
    /// Moves focus one step in `direction`. Answers whether focus moved.
    pub fn move_focus(&self, direction: FocusDirection) -> bool {
        let Some(target) = next_target(direction) else {
            return false;
        };
        focus_dispatch::request_focus_in_context(target)
    }

    /// Drops focus from whatever holds it, and answers whether one held it.
    /// Compose takes a `force` flag here for targets that refuse to give focus
    /// up; no target in Cranpose refuses, so this always clears.
    pub fn clear_focus(&self) -> bool {
        focus_dispatch::clear_active_focus()
    }
}

/// CompositionLocal carrying the [`FocusManager`], as `LocalFocusManager` does
/// in Compose. The same instance comes back on every call.
pub fn local_focus_manager() -> CompositionLocal<FocusManager> {
    thread_local! {
        static LOCAL_FOCUS_MANAGER: RefCell<Option<CompositionLocal<FocusManager>>> =
            const { RefCell::new(None) };
    }

    LOCAL_FOCUS_MANAGER.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| cranpose_core::compositionLocalOf(FocusManager::default))
            .clone()
    })
}

/// Moves focus onto `node_id` when it holds a focus target. A platform's
/// accessibility layer calls this when a screen reader lands on a control, so
/// the app's focus follows the reader's.
pub fn request_focus_from_platform(node_id: NodeId) -> bool {
    focus_dispatch::request_focus_in_context(node_id)
}

fn next_target(direction: FocusDirection) -> Option<NodeId> {
    with_focus_order(|order| {
        if order.is_empty() {
            return None;
        }
        let active = focus_dispatch::active_focus_target();
        let current = active.and_then(|node_id| order.iter().position(|e| e.node_id == node_id));

        match direction {
            FocusDirection::Next | FocusDirection::Enter => Some(step(order, current, 1)),
            FocusDirection::Previous => Some(step(order, current, -1)),
            FocusDirection::Exit => None,
            FocusDirection::Up
            | FocusDirection::Down
            | FocusDirection::Left
            | FocusDirection::Right => current.and_then(|index| nearest(order, index, direction)),
        }
    })
}

fn step(order: &[FocusEntry], current: Option<usize>, delta: isize) -> NodeId {
    let count = order.len() as isize;
    let index = match current {
        Some(index) => (index as isize + delta).rem_euclid(count),
        None if delta > 0 => 0,
        None => count - 1,
    };
    order[index as usize].node_id
}

fn nearest(order: &[FocusEntry], from: usize, direction: FocusDirection) -> Option<NodeId> {
    let (from_x, from_y) = order[from].center();
    let mut best: Option<(f32, NodeId)> = None;

    for (index, entry) in order.iter().enumerate() {
        if index == from {
            continue;
        }
        let (x, y) = entry.center();
        let (along, across) = match direction {
            FocusDirection::Up => (from_y - y, (x - from_x).abs()),
            FocusDirection::Down => (y - from_y, (x - from_x).abs()),
            FocusDirection::Left => (from_x - x, (y - from_y).abs()),
            FocusDirection::Right => (x - from_x, (y - from_y).abs()),
            _ => continue,
        };
        if along <= 0.0 {
            continue;
        }
        let cost = along + across * 2.0;
        if best.is_none_or(|(best_cost, _)| cost < best_cost) {
            best = Some((cost, entry.node_id));
        }
    }

    best.map(|(_, node_id)| node_id)
}

#[cfg(test)]
#[path = "tests/focus_manager_tests.rs"]
mod tests;
