// `LiquidContextMenu` through the headless robot: a long press lifts the
// content at 1.15 times its size and opens the menu 16 points below it, or
// above it near the bottom edge; the release that ends the long press keeps
// the menu open; a tap on an action runs it and closes the menu; a tap
// elsewhere closes it and runs nothing; the lifted content comes back with
// its state;
// readers open the menu through the long press action.

use std::rc::Rc;

use cranpose_core::{mutableStateOf, remember};
use cranpose_liquid::prelude::*;
use cranpose_testing::robot_assertions::{assert_approx_eq, find_semantics_node};
use cranpose_ui::{
    Alignment, Modifier, Size,
    widgets::{Box, BoxSpec, Text},
};

use crate::reader::{Log, Robot, liquid_screen, node, nodes, tap, tap_node};

const TILE: Size = Size {
    width: 160.0,
    height: 100.0,
};

// A 160 by 100 tile whose context menu holds Copy, Share and Delete, in a
// 400 by 800 window. `alignment` places the tile. The tile counts its taps.
fn tile(alignment: Alignment) -> (Robot, Log) {
    liquid_screen(move |log| {
        let record = Rc::clone(log);
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec::default().content_alignment(alignment),
            move || {
                let record = Rc::clone(&record);
                LiquidContextMenu(
                    Modifier::empty().size(TILE),
                    LiquidShape::RoundedRect(16.0),
                    "Photo options",
                    move |scope| {
                        for (label, entry) in [("Copy", "copy"), ("Share", "share")] {
                            let record = Rc::clone(&record);
                            scope.item(LiquidMenuItem::new(label), move || {
                                record.borrow_mut().push(entry);
                            });
                        }
                        let record = Rc::clone(&record);
                        scope.item(LiquidMenuItem::new("Delete").destructive(), move || {
                            record.borrow_mut().push("delete");
                        });
                    },
                    || {
                        let taps = remember(|| mutableStateOf(0_u32)).with(|taps| *taps);
                        Box(
                            Modifier::empty()
                                .fill_max_size()
                                .clickable(move |_| taps.set(taps.get() + 1)),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                Text(
                                    format!("Taps {}", taps.get()),
                                    Modifier::empty(),
                                    liquid_typography().body,
                                );
                            },
                        );
                    },
                );
            },
        );
    })
}

// Holds the pointer on (`x`, `y`) past the long press time, then lifts it.
fn long_press(robot: &mut Robot, x: f32, y: f32) {
    robot.mouse_move(x, y);
    robot.mouse_down();
    for _ in 0..12 {
        robot.advance_time(50_000_000);
    }
    robot.mouse_up();
    robot.wait_for_motion();
}

fn shown(robot: &mut Robot, label: &str) -> usize {
    nodes(robot)
        .iter()
        .filter(|node| node.label.as_deref() == Some(label))
        .count()
}

#[test]
fn a_long_press_opens_the_menu_16_points_below_the_lifted_content() {
    let (mut robot, log) = tile(Alignment::CENTER);
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu starts closed");
    long_press(&mut robot, 200.0, 400.0);
    // The lifted tile spans 400 ± 57.5; the menu's rows start 10 points
    // below its top edge.
    let copy = node(&mut robot, "Copy").target_bounds();
    assert_approx_eq(
        copy.y,
        400.0 + TILE.height * 1.15 * 0.5 + 16.0 + 10.0,
        1.0,
        "Copy's row starts 26 points below the lifted tile",
    );
    let delete = node(&mut robot, "Delete").target_bounds();
    assert!(
        delete.y > copy.y,
        "the rows keep their order below the tile"
    );
    assert!(log.borrow().is_empty(), "the release ran no action");
}

#[test]
fn near_the_bottom_edge_the_menu_opens_above_the_lifted_content() {
    let (mut robot, _) = tile(Alignment::BOTTOM_CENTER);
    long_press(&mut robot, 200.0, 750.0);
    let delete = node(&mut robot, "Delete").target_bounds();
    let copy = node(&mut robot, "Copy").target_bounds();
    // The lifted tile keeps 16 points above the window's bottom edge.
    let lifted_top = 800.0 - 16.0 - TILE.height * 1.15;
    assert_approx_eq(
        delete.y + delete.height,
        lifted_top - 16.0 - 10.0,
        1.0,
        "Delete's row ends 26 points above the lifted tile",
    );
    assert!(
        copy.y < delete.y,
        "the rows keep their order above the tile"
    );
}

#[test]
fn a_tap_on_an_action_runs_it_and_closes_the_menu() {
    let (mut robot, log) = tile(Alignment::CENTER);
    long_press(&mut robot, 200.0, 400.0);
    tap_node(&mut robot, "Share");
    robot.wait_for_motion();
    assert_eq!(*log.borrow(), ["share"]);
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu closed");
}

#[test]
fn a_tap_outside_closes_the_menu_and_runs_nothing() {
    let (mut robot, log) = tile(Alignment::CENTER);
    long_press(&mut robot, 200.0, 400.0);
    tap(&mut robot, 20.0, 80.0);
    robot.wait_for_motion();
    assert_eq!(shown(&mut robot, "Copy"), 0, "the menu closed");
    assert!(log.borrow().is_empty(), "no action ran");
}

#[test]
fn the_lifted_content_keeps_its_state() {
    let (mut robot, _) = tile(Alignment::CENTER);
    tap(&mut robot, 200.0, 400.0);
    // The tile and its text both read the count.
    let at_rest = shown(&mut robot, "Taps 1");
    assert!(at_rest > 0, "a tap counts");
    // Readers see only the open menu, so the count is read once it closes.
    long_press(&mut robot, 200.0, 400.0);
    tap(&mut robot, 20.0, 80.0);
    robot.wait_for_motion();
    assert_eq!(
        shown(&mut robot, "Taps 1"),
        at_rest,
        "the tile came back once, with its count"
    );
    tap(&mut robot, 200.0, 400.0);
    assert!(shown(&mut robot, "Taps 2") > 0, "the tile takes taps again");
}

#[test]
fn readers_open_the_menu_through_the_long_press_action() {
    let (mut robot, _) = tile(Alignment::CENTER);
    let tree = robot.shell_mut().semantics_tree().expect("semantics");
    let owner = find_semantics_node(tree.root(), &|node| {
        node.details().on_long_click_label.as_deref() == Some("Photo options")
    })
    .expect("the tile offers its menu");
    let open = owner.details().on_long_click.clone().expect("long press");
    assert!(open.invoke(), "the tile took the long press");
    robot.wait_for_motion();
    assert!(shown(&mut robot, "Copy") > 0, "the menu opened");
}
