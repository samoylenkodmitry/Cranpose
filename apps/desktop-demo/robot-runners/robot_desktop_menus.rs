//! A menu bar and a tray icon from the composition: choosing a menu bar item
//! (on macOS through the native menu bar) or a tray menu item runs its
//! callback as an event handler, and a check item's mark follows the state
//! the app keeps.

use cranpose::{CheckItem, KeyShortcut, Menu, MenuBar, MenuEntry, MenuItem, Tray, TraySpec};
use cranpose_core::{mutableStateOf, remember};
use cranpose_testing::{find_in_semantics, find_text_exact};
use cranpose_ui::{ImageBitmap, KeyCode, Modifier, Modifiers, Text, TextStyle};

use crate::{robot_exit, robot_launch, text_input_robot_helpers};

/// Chooses `labels` and waits for the screen to show `expected`.
fn choose(robot: &cranpose::Robot, tray: bool, labels: &[&str], expected: &str) {
    let chosen = if tray {
        robot.choose_tray_menu_item(labels)
    } else {
        robot.choose_menu_item(labels)
    };
    if let Err(error) = chosen {
        robot_exit::fail(robot, &format!("choosing {labels:?} failed: {error}"));
    }
    let shown = text_input_robot_helpers::wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |element| find_text_exact(element, expected))
    });
    if shown.is_none() {
        robot_exit::fail(robot, &format!("{labels:?} did not lead to {expected:?}"));
    }
    println!("PASS: {labels:?} shows {expected:?}");
}

pub(crate) fn main() {
    env_logger::init();
    // A shown window: macOS installs the menu bar after the first presented
    // frame, so a headless app has none.
    robot_launch::launch("Desktop Menus", 400, 200)
        .with_headless(false)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            let ready = text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                find_in_semantics(robot, |element| find_text_exact(element, "counted 0, unmarked"))
            });
            if ready.is_none() {
                robot_exit::fail(&robot, "the screen never showed its first state");
            }
            choose(&robot, false, &["Tools", "Count"], "counted 1, unmarked");
            choose(&robot, false, &["Tools", "Options", "Mark"], "counted 1, marked");
            choose(&robot, false, &["Tools", "Options", "Mark"], "counted 1, unmarked");
            choose(&robot, true, &["Count"], "counted 2, unmarked");
            let _ = robot.exit();
        })
        .run(|| {
            let counted = remember(|| mutableStateOf(0u32)).with(|state| *state);
            let marked = remember(|| mutableStateOf(false)).with(|state| *state);
            let mark = if marked.get() { "marked" } else { "unmarked" };
            Text(
                format!("counted {}, {mark}", counted.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
            let count = move || counted.set(counted.get() + 1);
            let command = Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            };
            MenuBar(vec![
                Menu::new("Tools")
                    .item(MenuItem::new("Count", count).shortcut(KeyShortcut::new(KeyCode::K, command)))
                    .separator()
                    .submenu(Menu::new("Options").check(CheckItem::new(
                        "Mark",
                        marked.get(),
                        move |checked| marked.set(checked),
                    ))),
            ]);
            match ImageBitmap::from_rgba8(2, 2, vec![255; 16]) {
                Ok(icon) => Tray(
                    TraySpec::new(icon, "Desktop Menus")
                        .menu(vec![MenuEntry::Item(MenuItem::new("Count", count))]),
                ),
                Err(error) => log::warn!("the tray icon is unusable: {error}"),
            }
        });
}
