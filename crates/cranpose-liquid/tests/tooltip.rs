//! `LiquidTooltipBox` through the headless robot: the tooltip shows while
//! the pointer rests on its control, while the control has keyboard focus,
//! and for a moment after a long press that does not click the control; a
//! screen reader asks for it by name; and it sits above the control, inside
//! the window, or below the control at the window's top.

use std::{cell::Cell, rc::Rc};

use cranpose_liquid::prelude::*;
use cranpose_testing::{RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, FocusRequester, Modifier, Point, Rect, SemanticsNode, Size,
};

const WIDTH: f32 = 300.0;
const CONTROL: f32 = 48.0;

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    clicks: Rc<Cell<u32>>,
    control: FocusRequester,
    other: FocusRequester,
}

/// A control with a tooltip at `at`, and another control below.
fn screen(at: Point) -> Screen {
    let clicks = Rc::new(Cell::new(0));
    let (control, other) = (FocusRequester::new(), FocusRequester::new());
    let (clicked, built, built_other) = (Rc::clone(&clicks), control.clone(), other.clone());
    let mut robot = create_headless_robot_test(WIDTH as u32, 300, move || {
        let clicked = Rc::clone(&clicked);
        let built = built.clone();
        let built_other = built_other.clone();
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let clicked = Rc::clone(&clicked);
            let built = built.clone();
            let built_other = built_other.clone();
            Column(
                Modifier::empty().padding_each(at.x, at.y, 0.0, 0.0),
                ColumnSpec::default(),
                move || {
                    let clicked = Rc::clone(&clicked);
                    let built = built.clone();
                    LiquidTooltipBox("Delete", "Show tooltip", Modifier::empty(), move || {
                        let clicked = Rc::clone(&clicked);
                        Box(
                            Modifier::empty()
                                .size(Size::new(CONTROL, CONTROL))
                                .focus_requester(&built)
                                .clickable(move |_| clicked.set(clicked.get() + 1)),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                    Box(
                        Modifier::empty()
                            .size(Size::new(CONTROL, CONTROL))
                            .focus_requester(&built_other)
                            .focus_target(),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        });
    });
    robot.wait_for_idle();
    Screen {
        robot,
        clicks,
        control,
        other,
    }
}

impl Screen {
    fn tooltip(&mut self) -> Option<Rect> {
        self.robot.find_by_text("Delete").bounds()
    }

    fn hold(&mut self, at: Point, millis: u64) {
        self.robot.mouse_move(at.x, at.y);
        self.robot.mouse_down();
        for _ in 0..millis / 50 {
            self.robot.advance_time(50_000_000);
        }
    }
}

const BELOW_TOP: Point = Point { x: 100.0, y: 100.0 };

fn control_center(at: Point) -> Point {
    Point::new(at.x + CONTROL / 2.0, at.y + CONTROL / 2.0)
}

#[test]
fn the_tooltip_shows_while_the_pointer_rests_on_the_control() {
    let mut screen = screen(BELOW_TOP);
    assert!(screen.tooltip().is_none());
    let center = control_center(BELOW_TOP);
    screen.robot.mouse_move(center.x, center.y);
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_some());
    screen.robot.mouse_move(5.0, 290.0);
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_none());
}

#[test]
fn the_tooltip_shows_while_the_control_has_keyboard_focus() {
    let mut screen = screen(BELOW_TOP);
    screen.control.request_focus().expect("composed control");
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_some());
    screen.other.request_focus().expect("composed control");
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_none());
}

#[test]
fn a_long_press_shows_the_tooltip_for_a_moment_without_a_click() {
    let mut screen = screen(BELOW_TOP);
    let center = control_center(BELOW_TOP);
    screen.hold(center, 600);
    screen.robot.mouse_up();
    // Away from the control, so hover does not keep it.
    screen.robot.mouse_move(5.0, 290.0);
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_some());
    assert_eq!(screen.clicks.get(), 0, "the long press did not click");
    screen.robot.wait_for_motion();
    assert!(
        screen.tooltip().is_none(),
        "the tooltip went after its time"
    );
}

#[test]
fn a_tap_clicks_the_control() {
    let mut screen = screen(BELOW_TOP);
    let center = control_center(BELOW_TOP);
    screen.robot.click_at(center.x, center.y);
    assert_eq!(screen.clicks.get(), 1);
}

fn find_long_click(node: &SemanticsNode) -> Option<(String, cranpose_ui::SemanticsLongClick)> {
    let details = node.details();
    details
        .on_long_click
        .clone()
        .zip(details.on_long_click_label.clone())
        .map(|(action, label)| (label, action))
        .or_else(|| node.children.iter().find_map(find_long_click))
}

#[test]
fn a_screen_reader_asks_for_the_tooltip_by_name() {
    let mut screen = screen(BELOW_TOP);
    screen.robot.shell_mut().set_semantics_enabled(true);
    screen.robot.wait_for_idle();
    let (label, action) = screen
        .robot
        .shell_mut()
        .semantics_tree()
        .and_then(|tree| find_long_click(tree.root()))
        .expect("the control offers its tooltip");
    assert_eq!(label, "Show tooltip");
    assert!(action.invoke());
    screen.robot.wait_for_idle();
    assert!(screen.tooltip().is_some());
}

fn tooltip_at(at: Point) -> Rect {
    let mut screen = screen(at);
    screen.control.request_focus().expect("composed control");
    screen.robot.wait_for_idle();
    screen
        .tooltip()
        .expect("the focused control shows its tooltip")
}

#[test]
fn the_tooltip_sits_above_its_control_and_centred_on_it() {
    let tooltip = tooltip_at(BELOW_TOP);
    assert!(tooltip.y + tooltip.height <= BELOW_TOP.y, "{tooltip:?}");
    let center = control_center(BELOW_TOP);
    assert!(
        (tooltip.x + tooltip.width / 2.0 - center.x).abs() < 1.0,
        "{tooltip:?}"
    );
}

#[test]
fn the_tooltip_stays_inside_the_window_and_goes_below_at_its_top() {
    let corner = Point::new(WIDTH - CONTROL, 0.0);
    let tooltip = tooltip_at(corner);
    assert!(tooltip.x + tooltip.width <= WIDTH, "{tooltip:?}");
    assert!(tooltip.y >= CONTROL, "below the control: {tooltip:?}");
}
