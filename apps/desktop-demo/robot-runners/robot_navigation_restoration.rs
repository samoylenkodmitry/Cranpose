//! Navigation in a desktop window: a screen's saved count and its list's
//! scroll position come back when the screen above it is popped, as
//! `NavHost` keeps each entry's saveable state while it is covered.

use cranpose::Robot;
use cranpose_foundation::lazy::LazyListScope;
use cranpose_navigation::{NavController, NavHost, rememberNavController, rememberSaveableLazyListState};
use cranpose_services::{Saver, rememberSaveable};
use cranpose_testing::{find_in_semantics, find_text_exact};
use cranpose_ui::{
    Button, ButtonSpec, Column, ColumnSpec, Modifier, Text, TextStyle,
    widgets::{LazyColumn, LazyColumnSpec},
};

use crate::{robot_exit, robot_launch, text_input_robot_helpers};

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    Detail(u32),
}

fn shown(robot: &Robot, text: &str) -> Option<(f32, f32)> {
    find_in_semantics(robot, |element| find_text_exact(element, text))
        .map(|(x, y, width, height)| (x + width / 2.0, y + height / 2.0))
}

fn wait_for(robot: &Robot, text: &str, step: &str) -> (f32, f32) {
    let found = text_input_robot_helpers::wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |element| find_text_exact(element, text))
    });
    match found {
        Some((x, y, width, height)) => (x + width / 2.0, y + height / 2.0),
        None => robot_exit::fail(robot, &format!("{step}: {text:?} never showed")),
    }
}

fn click(robot: &Robot, text: &str, step: &str) {
    let (x, y) = wait_for(robot, text, step);
    if let Err(error) = robot.click(x, y) {
        robot_exit::fail(robot, &format!("{step}: clicking {text:?} failed: {error}"));
    }
}

#[cranpose_macros::composable]
fn Home(nav: NavController<Screen>) {
    let taps = rememberSaveable("taps", Saver::of_display(), || 0_u32);
    let list = rememberSaveableLazyListState("list");
    Column(Modifier::empty().fill_max_size(), ColumnSpec::default(), move || {
        Text(format!("taps {}", taps.get()), Modifier::empty(), TextStyle::default());
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || taps.set(taps.get() + 1),
            || {
                Text("Tap", Modifier::empty(), TextStyle::default());
            },
        );
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || nav.navigate(Screen::Detail(7)),
            || {
                Text("Open 7", Modifier::empty(), TextStyle::default());
            },
        );
        LazyColumn(
            Modifier::empty().fill_max_width().height(160.0).test_tag("list"),
            list,
            LazyColumnSpec::new(),
            |scope| {
                scope.items(50, |index| {
                    Text(
                        format!("item {index}"),
                        Modifier::empty().height(24.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
}

#[cranpose_macros::composable]
fn Detail(nav: NavController<Screen>, id: u32) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(format!("detail {id}"), Modifier::empty(), TextStyle::default());
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || {
                nav.pop_back_stack();
            },
            || {
                Text("Back", Modifier::empty(), TextStyle::default());
            },
        );
    });
}

pub(crate) fn main() {
    env_logger::init();
    robot_launch::launch("Navigation and Restoration", 420, 320)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            wait_for(&robot, "taps 0", "first frame");
            for taps in 1..=3 {
                click(&robot, "Tap", "tap");
                wait_for(&robot, &format!("taps {taps}"), "the count moved");
            }

            let (x, y) = wait_for(&robot, "item 0", "the list");
            if let Err(error) = robot.mouse_move(x, y + 40.0) {
                robot_exit::fail(&robot, &format!("moving over the list failed: {error}"));
            }
            for _ in 0..40 {
                if shown(&robot, "item 0").is_none() {
                    break;
                }
                let _ = robot.mouse_scroll(0.0, -120.0);
                let _ = robot.wait_for_idle();
            }
            if shown(&robot, "item 0").is_some() {
                robot_exit::fail(&robot, "the list never scrolled past its first item");
            }
            println!("PASS: the home screen counted and scrolled");

            click(&robot, "Open 7", "open the detail");
            wait_for(&robot, "detail 7", "the detail screen");
            click(&robot, "Back", "back to home");
            wait_for(&robot, "taps 3", "the count came back");
            if shown(&robot, "item 0").is_some() {
                robot_exit::fail(&robot, "the list came back at its start, not where it was");
            }
            println!("PASS: a popped screen's cover leaves the home count and scroll as they were");
            let _ = robot.exit();
        })
        .run(|| {
            let nav = rememberNavController(Screen::Home);
            NavHost(nav, move |screen| match screen {
                Screen::Home => Home(nav),
                Screen::Detail(id) => Detail(nav, id),
            });
        });
}
