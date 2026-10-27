use cranpose_core::{CompositionLocal, NodeId};
use cranpose_ui_graphics::Rect;

use crate::{
    focus_dispatch,
    focus_order::{FocusEntry, with_focus_order},
    modifier::FocusDirection,
    two_dimensional_focus_search::{Arrow, entry_point, find_best_candidate},
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
        let moved = focus_dispatch::request_focus_in_context(target);
        if moved {
            focus_dispatch::note_focus_rect(target);
        }
        moved
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
    crate::environment_locals::cached_local(
        |locals| &locals.focus_manager,
        || cranpose_core::compositionLocalOf(FocusManager::default),
    )
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

        if let Some(arrow) = Arrow::of(direction) {
            return arrow_target(order, active, arrow);
        }
        match direction {
            FocusDirection::Next | FocusDirection::Enter => Some(step(order, current, 1)),
            FocusDirection::Previous => Some(step(order, current, -1)),
            FocusDirection::Exit
            | FocusDirection::Up
            | FocusDirection::Down
            | FocusDirection::Left
            | FocusDirection::Right => None,
        }
    })
}

// The target an arrow reaches in `order` from the focused target `active`.
// With the focused target gone, the arrow goes to the target that took its
// place, as a list gives focus to the item that moved into the removed one's
// row, or else searches from that place. With nothing focused before, it
// enters from the arrow's edge.
pub(crate) fn arrow_target(
    order: &[FocusEntry],
    active: Option<NodeId>,
    arrow: Arrow,
) -> Option<NodeId> {
    if let Some(focused) = order.iter().find(|entry| Some(entry.node_id) == active) {
        let candidates = order.iter().filter(|entry| Some(entry.node_id) != active);
        return find_best_candidate(candidates, focused.rect, arrow);
    }
    if let Some(last) = focus_dispatch::last_focus_rect() {
        return order
            .iter()
            .find(|entry| covers_center(entry.rect, last))
            .map(|entry| entry.node_id)
            .or_else(|| find_best_candidate(order, last, arrow));
    }
    find_best_candidate(order, entry_point(order, arrow)?, arrow)
}

fn covers_center(rect: Rect, of: Rect) -> bool {
    let (x, y) = (of.x + of.width * 0.5, of.y + of.height * 0.5);
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
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

#[cfg(test)]
#[path = "tests/focus_manager_tests.rs"]
mod tests;
