//! Presses after a frame that composed or laid out again: the scene collects
//! its hit targets again only when the update changed one or moved one, and
//! a press must still find each control where it now is, with the handler
//! the last composition gave it.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Size, Text, TextStyle};

/// A control of 100 x 50 that counts its clicks in `clicks`.
fn control(clicks: Rc<Cell<u32>>) {
    Box(
        Modifier::empty()
            .size(Size::new(100.0, 50.0))
            .clickable(move |_| clicks.set(clicks.get() + 1)),
        BoxSpec::default(),
        || {},
    );
}

/// A column of a sibling that reads a state and a [`control`] below it.
struct ControlBelow<T: Clone + 'static> {
    robot: RobotTestRule<TestRenderer>,
    /// The clicks the control counted.
    clicks: Rc<Cell<u32>>,
    /// The state the sibling reads, once composed.
    state: Rc<Cell<Option<MutableState<T>>>>,
}

fn control_below<T: Clone + PartialEq + 'static>(
    initial: T,
    sibling: impl Fn(MutableState<T>) + 'static,
) -> ControlBelow<T> {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let state: Rc<Cell<Option<MutableState<T>>>> = Rc::default();
    let installed = Rc::clone(&state);
    let sibling = Rc::new(sibling);
    let robot = create_headless_robot_test(300, 400, move || {
        let value = rememberMutableStateOf(|| initial.clone());
        installed.set(Some(value));
        let counted = Rc::clone(&counted);
        let sibling = Rc::clone(&sibling);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            sibling(value);
            control(Rc::clone(&counted));
        });
    });
    ControlBelow {
        robot,
        clicks,
        state,
    }
}

#[test]
fn a_click_follows_a_control_that_a_taller_sibling_pushed_down() {
    let ControlBelow {
        mut robot,
        clicks,
        state: height,
    } = control_below(20.0_f32, |height| {
        Box(
            Modifier::empty().size(Size::new(100.0, height.get())),
            BoxSpec::default(),
            || {},
        );
    });
    robot.wait_for_idle();
    height.get().expect("composed").set(120.0);
    robot.wait_for_idle();
    robot.click_at(50.0, 45.0);
    assert_eq!(clicks.get(), 0, "the control left this place");
    robot.click_at(50.0, 145.0);
    assert_eq!(clicks.get(), 1, "the control is here now");
}

#[test]
fn a_click_reaches_a_control_after_a_sibling_text_changed() {
    let ControlBelow {
        mut robot,
        clicks,
        state: reading,
    } = control_below(10u32, |value| {
        // The text keeps its height, so the control stays where it is.
        Text(
            format!("Reading {}", value.get()),
            Modifier::empty().height(20.0),
            TextStyle::default(),
        );
    });
    robot.wait_for_idle();
    for next in 11..14 {
        reading.get().expect("composed").set(next);
        robot.wait_for_idle();
        robot.click_at(50.0, 45.0);
    }
    assert_eq!(clicks.get(), 3);
}

#[test]
fn a_click_runs_the_handler_of_the_last_composition() {
    let first = Rc::new(Cell::new(0));
    let second = Rc::new(Cell::new(0));
    let target: Rc<Cell<Option<MutableState<bool>>>> = Rc::default();
    let installed = Rc::clone(&target);
    let (to_first, to_second) = (Rc::clone(&first), Rc::clone(&second));
    let mut robot = create_headless_robot_test(300, 400, move || {
        let use_second = rememberMutableStateOf(|| false);
        installed.set(Some(use_second));
        control(if use_second.get() {
            Rc::clone(&to_second)
        } else {
            Rc::clone(&to_first)
        });
    });
    robot.wait_for_idle();
    robot.click_at(50.0, 25.0);
    target.get().expect("composed").set(true);
    robot.wait_for_idle();
    robot.click_at(50.0, 25.0);
    assert_eq!((first.get(), second.get()), (1, 1));
}
