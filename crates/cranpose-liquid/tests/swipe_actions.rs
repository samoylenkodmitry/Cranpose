// `LiquidSwipeActions` through the headless robot: a swipe toward the
// leading edge shows the trailing actions, the first at the edge, all as wide
// as the widest; a tap on one runs it and closes the row; a tap on the open
// row's content closes it; readers find every action among the row's custom
// actions.

use std::rc::Rc;

use cranpose_liquid::prelude::*;
use cranpose_testing::robot_assertions::{assert_approx_eq, find_semantics_node};
use cranpose_ui::{
    Alignment, Modifier, SemanticsNode,
    widgets::{Box, BoxSpec, Text},
};

use crate::reader::{Log, Robot, liquid_screen, node, nodes, tap_node};

const ROW_Y: f32 = 122.0;

// A 300 by 44 row at (50, 100) in a 400 by 800 window, with Delete and Flag
// at its trailing edge and Pin at its leading edge.
fn row() -> (Robot, Log) {
    liquid_screen(move |log| {
        let record = Rc::clone(log);
        Box(
            Modifier::empty()
                .fill_max_size()
                .padding_each(50.0, 100.0, 50.0, 0.0),
            BoxSpec::default().content_alignment(Alignment::TOP_START),
            move || {
                let colors = liquid_colors();
                let (delete, flag, pin) =
                    (Rc::clone(&record), Rc::clone(&record), Rc::clone(&record));
                LiquidSwipeActions(
                    Modifier::empty().width(300.0).height(44.0),
                    move |scope| {
                        let (delete, flag, pin) =
                            (Rc::clone(&delete), Rc::clone(&flag), Rc::clone(&pin));
                        scope.trailing("Delete", colors.destructive, move || {
                            delete.borrow_mut().push("delete")
                        });
                        scope.trailing("Flag", colors.warning, move || {
                            flag.borrow_mut().push("flag")
                        });
                        scope.leading("Pin", colors.accent, move || pin.borrow_mut().push("pin"));
                    },
                    || {
                        Text(
                            "Content",
                            Modifier::empty().padding(12.0),
                            liquid_typography().body,
                        );
                    },
                );
            },
        );
    })
}

fn shown(robot: &mut Robot, label: &str) -> bool {
    nodes(robot).iter().any(|node| {
        node.label.as_deref() == Some(label)
            // The pill takes taps, so its bounds follow the drag.
            && node.touch_bounds.is_some_and(|bounds| bounds.x + bounds.width > 50.5 && bounds.x < 349.5)
    })
}

#[test]
fn a_swipe_shows_the_trailing_actions_with_the_first_at_the_edge() {
    let (mut robot, _) = row();
    assert!(
        !shown(&mut robot, "Delete"),
        "a closed row hides its actions"
    );
    robot.drag(320.0, ROW_Y, 120.0, ROW_Y);
    let delete = node(&mut robot, "Delete").target_bounds();
    let flag = node(&mut robot, "Flag").target_bounds();
    assert_approx_eq(
        delete.x + delete.width,
        340.0,
        0.01,
        "Delete ends 10 from the edge",
    );
    assert_approx_eq(
        delete.x - flag.x - flag.width,
        10.0,
        0.01,
        "the pills sit 10 apart",
    );
    assert_approx_eq(
        flag.width,
        delete.width,
        0.01,
        "the pills share the widest label's width",
    );
    // The 4.5 point insets round to whole pixels at this density.
    assert_approx_eq(
        delete.height,
        35.0,
        1.0,
        "a pill is 9 points shorter than the row",
    );
    assert!(!shown(&mut robot, "Pin"), "the leading action stays hidden");
}

#[test]
fn a_tap_on_an_action_runs_it_and_closes_the_row() {
    let (mut robot, log) = row();
    robot.drag(320.0, ROW_Y, 120.0, ROW_Y);
    tap_node(&mut robot, "Flag");
    robot.wait_for_motion();
    assert_eq!(*log.borrow(), ["flag"]);
    assert!(!shown(&mut robot, "Flag"), "the row closed");

    robot.drag(80.0, ROW_Y, 280.0, ROW_Y);
    assert!(shown(&mut robot, "Pin"), "a swipe the other way shows Pin");
    tap_node(&mut robot, "Pin");
    robot.wait_for_motion();
    assert_eq!(*log.borrow(), ["flag", "pin"]);
}

#[test]
fn a_tap_on_the_open_content_closes_the_row() {
    let (mut robot, log) = row();
    robot.drag(320.0, ROW_Y, 120.0, ROW_Y);
    assert!(shown(&mut robot, "Delete"));
    robot.click_at(70.0, ROW_Y);
    robot.wait_for_motion();
    assert!(!shown(&mut robot, "Delete"), "the row closed");
    assert!(log.borrow().is_empty(), "no action ran");
}

#[test]
fn readers_find_every_action_on_the_row() {
    let (mut robot, log) = row();
    let tree = robot.shell_mut().semantics_tree().expect("semantics");
    let labels = |node: &SemanticsNode| {
        node.details()
            .custom_actions
            .iter()
            .map(|action| action.label.clone())
            .collect::<Vec<_>>()
    };
    let row = find_semantics_node(tree.root(), &|node| !labels(node).is_empty())
        .expect("the row offers its actions");
    assert_eq!(labels(row), ["Delete", "Flag", "Pin"]);
    let flag = row
        .details()
        .custom_actions
        .iter()
        .find(|action| action.label == "Flag")
        .cloned()
        .expect("a Flag action");
    flag.invoke();
    assert_eq!(*log.borrow(), ["flag"]);
}
