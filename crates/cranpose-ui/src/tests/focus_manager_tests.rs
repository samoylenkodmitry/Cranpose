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
