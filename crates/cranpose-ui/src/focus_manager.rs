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
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use cranpose_foundation::FocusState;
    use cranpose_ui_graphics::Rect;

    use super::*;
    use crate::{
        focus_dispatch::{FocusTargetHandle, register_focus_target},
        focus_order::set_focus_order,
    };

    struct Target {
        states: RefCell<Vec<FocusState>>,
    }

    impl Target {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                states: RefCell::new(Vec::new()),
            })
        }
    }

    impl FocusTargetHandle for Target {
        fn set_focus_state(&self, state: FocusState) {
            self.states.borrow_mut().push(state);
        }
    }

    fn row(node_id: NodeId, x: f32, y: f32) -> FocusEntry {
        FocusEntry {
            node_id,
            rect: Rect {
                x,
                y,
                width: 100.0,
                height: 40.0,
            },
        }
    }

    fn targets(ids: &[NodeId]) -> Vec<Rc<Target>> {
        ids.iter()
            .map(|node_id| {
                let target = Target::new();
                register_focus_target(*node_id, Rc::clone(&target) as Rc<dyn FocusTargetHandle>);
                target
            })
            .collect()
    }

    #[test]
    fn next_takes_the_first_target_when_nothing_holds_focus() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _targets = targets(&[1, 2, 3]);
        set_focus_order(vec![
            row(1, 0.0, 0.0),
            row(2, 0.0, 50.0),
            row(3, 0.0, 100.0),
        ]);

        assert!(FocusManager.move_focus(FocusDirection::Next));
        assert_eq!(focus_dispatch::active_focus_target(), Some(1));
    }

    #[test]
    fn next_and_previous_walk_the_order_and_wrap() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _targets = targets(&[1, 2, 3]);
        set_focus_order(vec![
            row(1, 0.0, 0.0),
            row(2, 0.0, 50.0),
            row(3, 0.0, 100.0),
        ]);

        FocusManager.move_focus(FocusDirection::Next);
        FocusManager.move_focus(FocusDirection::Next);
        assert_eq!(focus_dispatch::active_focus_target(), Some(2));

        FocusManager.move_focus(FocusDirection::Next);
        assert_eq!(focus_dispatch::active_focus_target(), Some(3));

        FocusManager.move_focus(FocusDirection::Next);
        assert_eq!(
            focus_dispatch::active_focus_target(),
            Some(1),
            "Next past the last target comes back to the first"
        );

        FocusManager.move_focus(FocusDirection::Previous);
        assert_eq!(
            focus_dispatch::active_focus_target(),
            Some(3),
            "Previous from the first target goes to the last"
        );
    }

    #[test]
    fn a_direction_takes_the_nearest_target_that_way() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _targets = targets(&[1, 2, 3, 4]);
        set_focus_order(vec![
            row(1, 0.0, 0.0),
            row(2, 200.0, 0.0),
            row(3, 0.0, 200.0),
            row(4, 0.0, 600.0),
        ]);

        FocusManager.move_focus(FocusDirection::Next);
        assert_eq!(focus_dispatch::active_focus_target(), Some(1));

        assert!(FocusManager.move_focus(FocusDirection::Right));
        assert_eq!(focus_dispatch::active_focus_target(), Some(2));

        assert!(FocusManager.move_focus(FocusDirection::Left));
        assert_eq!(focus_dispatch::active_focus_target(), Some(1));

        assert!(FocusManager.move_focus(FocusDirection::Down));
        assert_eq!(
            focus_dispatch::active_focus_target(),
            Some(3),
            "Down takes the nearer of the two targets below, not the far one"
        );

        assert!(FocusManager.move_focus(FocusDirection::Up));
        assert_eq!(
            focus_dispatch::active_focus_target(),
            Some(1),
            "Up from the lower row takes the target straight above, not the one off to the side"
        );
    }

    #[test]
    fn a_direction_with_nothing_that_way_leaves_focus_alone() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _targets = targets(&[1, 2]);
        set_focus_order(vec![row(1, 0.0, 0.0), row(2, 0.0, 50.0)]);

        FocusManager.move_focus(FocusDirection::Next);
        assert_eq!(focus_dispatch::active_focus_target(), Some(1));

        assert!(!FocusManager.move_focus(FocusDirection::Up));
        assert_eq!(focus_dispatch::active_focus_target(), Some(1));
    }

    #[test]
    fn clear_focus_drops_the_active_target() {
        let _app_context = crate::render_state::app_context_test_scope();
        let targets = targets(&[1, 2]);
        set_focus_order(vec![row(1, 0.0, 0.0), row(2, 0.0, 50.0)]);

        FocusManager.move_focus(FocusDirection::Next);
        assert!(FocusManager.clear_focus());
        assert_eq!(focus_dispatch::active_focus_target(), None);
        assert_eq!(
            targets[0].states.borrow().last(),
            Some(&FocusState::Inactive)
        );
        assert!(!FocusManager.clear_focus());
    }

    #[test]
    fn a_move_without_any_target_answers_no() {
        let _app_context = crate::render_state::app_context_test_scope();
        set_focus_order(Vec::new());

        assert!(!FocusManager.move_focus(FocusDirection::Next));
        assert_eq!(focus_dispatch::active_focus_target(), None);
    }
}
