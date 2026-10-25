//! Presses after a frame that composed or laid out again: the scene collects
//! its hit targets again only when the update changed one or moved one, and
//! a press must still find each control where it now is, with the handler
//! the last composition gave it.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, Modifier, Size, Text, TextStyle};

/// A control of 100 x 50 below a spacer whose height is a state.
struct ControlBelowSpacer {
    robot: RobotTestRule<TestRenderer>,
    /// The clicks the control counted.
    clicks: Rc<Cell<u32>>,
    /// The spacer's height, once composed.
    height: Rc<Cell<Option<MutableState<f32>>>>,
}

fn control_below_spacer() -> ControlBelowSpacer {
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let height: Rc<Cell<Option<MutableState<f32>>>> = Rc::default();
    let installed = Rc::clone(&height);
    let robot = create_headless_robot_test(300, 400, move || {
        let spacer = rememberMutableStateOf(|| 20.0_f32);
        installed.set(Some(spacer));
        let counted = Rc::clone(&counted);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Box(
                Modifier::empty().size(Size::new(100.0, spacer.get())),
                BoxSpec::default(),
                || {},
            );
            let counted = Rc::clone(&counted);
            Box(
                Modifier::empty()
                    .size(Size::new(100.0, 50.0))
                    .clickable(move |_| counted.set(counted.get() + 1)),
                BoxSpec::default(),
                || {},
            );
        });
    });
    ControlBelowSpacer {
        robot,
        clicks,
        height,
    }
}

#[test]
fn a_click_follows_a_control_that_a_taller_sibling_pushed_down() {
    let ControlBelowSpacer {
        mut robot,
        clicks,
        height,
    } = control_below_spacer();
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
    let clicks = Rc::new(Cell::new(0));
    let counted = Rc::clone(&clicks);
    let reading: Rc<Cell<Option<MutableState<u32>>>> = Rc::default();
    let installed = Rc::clone(&reading);
    let mut robot = create_headless_robot_test(300, 400, move || {
        let value = rememberMutableStateOf(|| 10u32);
        installed.set(Some(value));
        let counted = Rc::clone(&counted);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            // The text keeps its height, so the control stays where it is.
            Text(
                format!("Reading {}", value.get()),
                Modifier::empty().height(20.0),
                TextStyle::default(),
            );
            let counted = Rc::clone(&counted);
            Box(
                Modifier::empty()
                    .size(Size::new(100.0, 50.0))
                    .clickable(move |_| counted.set(counted.get() + 1)),
                BoxSpec::default(),
                || {},
            );
        });
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
        let counter = if use_second.get() {
            Rc::clone(&to_second)
        } else {
            Rc::clone(&to_first)
        };
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 50.0))
                .clickable(move |_| counter.set(counter.get() + 1)),
            BoxSpec::default(),
            || {},
        );
    });
    robot.wait_for_idle();
    robot.click_at(50.0, 25.0);
    target.get().expect("composed").set(true);
    robot.wait_for_idle();
    robot.click_at(50.0, 25.0);
    assert_eq!((first.get(), second.get()), (1, 1));
}
