//! `Dialog` through the headless robot: a tap on the dialog's own surface
//! keeps it open, even where nothing on the surface is clickable, a tap on
//! the scrim around it dismisses it, and a screen reader dismisses it as the
//! back gesture does.

use std::{cell::Cell, rc::Rc};

use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, robot_assertions::find_semantics_node,
};
use cranpose_ui::{
    Box, BoxSpec, Color, Modifier, SemanticsDismiss, Size, Text, TextStyle,
    widgets::{Dialog, DialogSpec, DismissReason},
};

fn screen() -> (RobotTestRule<TestRenderer>, Rc<Cell<Option<DismissReason>>>) {
    screen_with(DialogSpec::default())
}

fn screen_with(spec: DialogSpec) -> (RobotTestRule<TestRenderer>, Rc<Cell<Option<DismissReason>>>) {
    let dismissed = Rc::new(Cell::new(None));
    let recorded = Rc::clone(&dismissed);
    let robot = create_headless_robot_test(300, 400, move || {
        let recorded = Rc::clone(&recorded);
        Dialog(
            spec,
            move |reason| recorded.set(Some(reason)),
            || {
                Box(
                    Modifier::empty()
                        .size(Size::new(200.0, 120.0))
                        .background(Color::WHITE),
                    BoxSpec::default(),
                    || {
                        Text("Title", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });
    (robot, dismissed)
}

#[test]
fn a_tap_on_the_surface_keeps_the_dialog_open() {
    let (mut robot, dismissed) = screen();
    let title = robot
        .find_by_text("Title")
        .center()
        .expect("the dialog shows its title");
    robot.click_at(title.x, title.y);
    robot.wait_for_idle();
    // The empty corner of the surface, away from the title.
    robot.click_at(150.0 + 90.0, 200.0 + 50.0);
    robot.wait_for_idle();
    assert_eq!(dismissed.get(), None);
}

#[test]
fn a_tap_on_the_scrim_dismisses_the_dialog() {
    let (mut robot, dismissed) = screen();
    robot.click_at(10.0, 10.0);
    robot.wait_for_idle();
    assert_eq!(dismissed.get(), Some(DismissReason::OutsideTap));
}

fn reader_dismiss(robot: &mut RobotTestRule<TestRenderer>) -> Option<SemanticsDismiss> {
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    robot.shell_mut().semantics_tree().and_then(|tree| {
        find_semantics_node(tree.root(), &|node| node.details().dismiss.is_some())
            .and_then(|node| node.details().dismiss.clone())
    })
}

#[test]
fn a_screen_reader_dismisses_the_dialog_as_the_back_gesture_does() {
    let (mut robot, dismissed) = screen();
    let dismiss = reader_dismiss(&mut robot).expect("the dialog offers a dismiss action");
    assert!(dismiss.invoke());
    assert_eq!(dismissed.get(), Some(DismissReason::BackRequested));
}

#[test]
fn a_dialog_the_user_must_answer_offers_a_reader_no_dismiss() {
    let (mut robot, _) = screen_with(DialogSpec::required());
    assert!(reader_dismiss(&mut robot).is_none());
}
