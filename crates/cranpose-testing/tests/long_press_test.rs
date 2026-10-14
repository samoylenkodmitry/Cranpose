//! `Modifier::on_long_press` through the headless robot: a press that rests
//! long enough fires it and keeps the button inside from clicking, a tap
//! clicks the button only, a press that moves first does neither, and a
//! screen reader asks for it by name.

use std::{cell::Cell, rc::Rc};

use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, robot_assertions::find_semantics_node,
};
use cranpose_ui::{Box, BoxSpec, Modifier, Size};

const LABEL: &str = "Show details";

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    long_presses: Rc<Cell<u32>>,
    clicks: Rc<Cell<u32>>,
}

/// A clickable button inside a box that takes a long press.
fn screen() -> Screen {
    let long_presses = Rc::new(Cell::new(0));
    let clicks = Rc::new(Cell::new(0));
    let (pressed, clicked) = (Rc::clone(&long_presses), Rc::clone(&clicks));
    let mut robot = create_headless_robot_test(300, 200, move || {
        let pressed = Rc::clone(&pressed);
        let clicked = Rc::clone(&clicked);
        Box(
            Modifier::empty().on_long_press(LABEL, move || pressed.set(pressed.get() + 1)),
            BoxSpec::default(),
            move || {
                let clicked = Rc::clone(&clicked);
                Box(
                    Modifier::empty()
                        .size(Size::new(100.0, 50.0))
                        .clickable(move |_| clicked.set(clicked.get() + 1)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    robot.wait_for_idle();
    Screen {
        robot,
        long_presses,
        clicks,
    }
}

impl Screen {
    /// Presses the button, holds it for `held_millis` in frames, moves it
    /// by `moved`, and lets go.
    fn press(&mut self, held_millis: u64, moved: f32) {
        self.robot.mouse_move(50.0, 25.0);
        self.robot.mouse_down();
        if moved > 0.0 {
            self.robot.mouse_move(50.0 + moved, 25.0);
        }
        for _ in 0..held_millis / 50 {
            self.robot.advance_time(50_000_000);
        }
        self.robot.mouse_up();
        self.robot.wait_for_idle();
    }
}

#[test]
fn a_rested_press_long_presses_and_the_button_inside_does_not_click() {
    let mut screen = screen();
    screen.press(600, 0.0);
    assert_eq!(screen.long_presses.get(), 1);
    assert_eq!(screen.clicks.get(), 0);
}

#[test]
fn a_tap_clicks_the_button_only() {
    let mut screen = screen();
    screen.press(100, 0.0);
    assert_eq!(screen.long_presses.get(), 0);
    assert_eq!(screen.clicks.get(), 1);
}

#[test]
fn a_press_that_moves_first_does_not_long_press() {
    let mut screen = screen();
    screen.press(600, 30.0);
    assert_eq!(screen.long_presses.get(), 0);
}

#[test]
fn a_screen_reader_long_presses_by_name() {
    let mut screen = screen();
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    let tree = screen
        .robot
        .shell_mut()
        .semantics_tree()
        .expect("semantics");
    let node = find_semantics_node(tree.root(), &|node| node.details().on_long_click.is_some())
        .expect("the box offers its long press");
    assert_eq!(node.details().on_long_click_label.as_deref(), Some(LABEL));
    let action = node.details().on_long_click.clone().expect("long press");
    assert!(action.invoke());
    assert_eq!(screen.long_presses.get(), 1);
}

#[test]
fn a_long_press_beside_a_click_on_one_control_does_not_click() {
    let long_presses = Rc::new(Cell::new(0));
    let clicks = Rc::new(Cell::new(0));
    let (pressed, clicked) = (Rc::clone(&long_presses), Rc::clone(&clicks));
    let robot = create_headless_robot_test(300, 200, move || {
        let pressed = Rc::clone(&pressed);
        let clicked = Rc::clone(&clicked);
        Box(
            Modifier::empty()
                .size(Size::new(100.0, 50.0))
                .clickable(move |_| clicked.set(clicked.get() + 1))
                .on_long_press(LABEL, move || pressed.set(pressed.get() + 1)),
            BoxSpec::default(),
            || {},
        );
    });
    let mut screen = Screen {
        robot,
        long_presses,
        clicks,
    };
    screen.robot.wait_for_idle();
    screen.press(600, 0.0);
    assert_eq!(screen.long_presses.get(), 1);
    assert_eq!(screen.clicks.get(), 0);
    screen.press(100, 0.0);
    assert_eq!(screen.clicks.get(), 1, "a tap still clicks");
}
