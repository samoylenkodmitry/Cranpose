//! A menu bar drawn in the window, the shell's menu bar on Linux: a click
//! on a title opens its menu and a click on an item runs it, a submenu opens
//! when the pointer rests on its item, the open menu takes the keyboard
//! focus (arrow keys, Return, Escape), and an item's shortcut runs it with
//! the menus closed.

use cranpose::{CheckItem, KeyShortcut, Menu, MenuItem, Robot, WindowMenuBar};
use cranpose_core::{mutableStateOf, remember};
use cranpose_testing::{find_in_semantics, find_text_exact};
use cranpose_ui::{KeyCode, Modifier, Modifiers, Text, TextStyle};

use crate::{robot_exit, robot_launch, text_input_robot_helpers};

fn wait_for(robot: &Robot, text: &str, step: &str) -> (f32, f32) {
    let found = text_input_robot_helpers::wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |element| find_text_exact(element, text))
    });
    match found {
        Some((x, y, width, height)) => (x + width / 2.0, y + height / 2.0),
        None => robot_exit::fail(robot, &format!("{step}: {text:?} never showed")),
    }
}

fn wait_gone(robot: &Robot, text: &str, step: &str) {
    for _ in 0..200 {
        if find_in_semantics(robot, |element| find_text_exact(element, text)).is_none() {
            return;
        }
        let _ = robot.wait_for_idle();
    }
    robot_exit::fail(robot, &format!("{step}: {text:?} stayed on the screen"));
}

fn click(robot: &Robot, text: &str, step: &str) {
    let (x, y) = wait_for(robot, text, step);
    if let Err(error) = robot.click(x, y) {
        robot_exit::fail(robot, &format!("{step}: clicking {text:?} failed: {error}"));
    }
}

pub(crate) fn main() {
    env_logger::init();
    robot_launch::launch("Window Menu Bar", 420, 260)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            wait_for(&robot, "counted 0, unmarked", "first frame");

            click(&robot, "Tools", "open Tools");
            click(&robot, "Count", "choose Count");
            wait_for(&robot, "counted 1, unmarked", "Count ran");
            wait_gone(&robot, "Count", "the menu closed after Count");
            println!("PASS: a click on an item runs it and closes the menu");

            click(&robot, "Tools", "open Tools again");
            let (x, y) = wait_for(&robot, "Options", "the submenu's item");
            if let Err(error) = robot.mouse_move(x, y) {
                robot_exit::fail(&robot, &format!("resting on Options failed: {error}"));
            }
            click(&robot, "Mark", "choose Mark in the submenu");
            wait_for(&robot, "counted 1, marked", "Mark ran");
            println!("PASS: a submenu opens beside its item");

            click(&robot, "Tools", "open Tools for Escape");
            wait_for(&robot, "Count", "the menu opened");
            if let Err(error) = robot.send_key("Escape") {
                robot_exit::fail(&robot, &format!("Escape failed: {error}"));
            }
            wait_gone(&robot, "Count", "Escape closed the menu");
            println!("PASS: Escape closes the menus");

            click(&robot, "Tools", "open Tools for the keyboard");
            wait_for(&robot, "Count", "the menu opened for the keyboard");
            for key in ["Down", "Return"] {
                if let Err(error) = robot.send_key(key) {
                    robot_exit::fail(&robot, &format!("{key} failed: {error}"));
                }
            }
            wait_for(&robot, "Mark", "Down and Return opened the submenu");
            if let Err(error) = robot.send_key("Escape") {
                robot_exit::fail(&robot, &format!("Escape failed: {error}"));
            }
            wait_gone(&robot, "Count", "Escape closed the menus");
            println!("PASS: the arrow keys move between lines and Return chooses");

            if let Err(error) = robot.send_key_with_modifiers("k", false, true, false, false) {
                robot_exit::fail(&robot, &format!("Ctrl+K failed: {error}"));
            }
            wait_for(&robot, "counted 2, marked", "the shortcut ran Count");
            println!("PASS: a shortcut runs its item");
            let _ = robot.exit();
        })
        .run(|| {
            let counted = remember(|| mutableStateOf(0u32)).with(|state| *state);
            let marked = remember(|| mutableStateOf(false)).with(|state| *state);
            let command = Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            };
            let menus = vec![
                Menu::new("Tools")
                    .item(
                        MenuItem::new("Count", move || counted.set(counted.get() + 1))
                            .shortcut(KeyShortcut::new(KeyCode::K, command)),
                    )
                    .separator()
                    .submenu(Menu::new("Options").check(CheckItem::new(
                        "Mark",
                        marked.get(),
                        move |checked| marked.set(checked),
                    ))),
                Menu::new("Help").item(MenuItem::new("About", || {})),
            ];
            WindowMenuBar(menus, move || {
                let mark = if marked.get() { "marked" } else { "unmarked" };
                Text(
                    format!("counted {}, {mark}", counted.get()),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            });
        });
}
