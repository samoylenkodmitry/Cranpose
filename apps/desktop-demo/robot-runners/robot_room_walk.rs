use std::time::Duration;

use crate::{output_paths, robot_controls_grid, robot_exit, robot_launch, robot_shot};

use cranpose::{PointerButton, Robot, SemanticElement};
use cranpose_testing::{click_button_in_semantics, find_text_in_semantics};

const STEP_IN: &str = "Step into the room";
/// Longer than the 0.9 s walk between the screen and the room.
const WALK: Duration = Duration::from_millis(1400);

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Room Walk ===");
    robot_exit::arm_timeout(120);
    let shots = output_paths::diagnostic_path("cranpose-robot-room");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }

    robot_launch::launch("Robot Room Walk", 1200, 800)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "room");
            // The globe spins on the flat screen, so the app does not go idle
            // until it stops.
            walk(&robot);
            save(&robot, &shots, "flat.png");
            if !click_button_in_semantics(&robot, "Stop the spin") {
                robot_exit::fail(&robot, "the globe's button is missing on the flat screen");
            }
            robot_shot::settle(&robot, 300);
            if find_text_in_semantics(&robot, "Spin the globe").is_none() {
                robot_exit::fail(
                    &robot,
                    "a click on the globe's button, where it shows on the sphere, missed it",
                );
            }
            let flat = step_in_button(&robot);

            if !click_button_in_semantics(&robot, STEP_IN) {
                robot_exit::fail(&robot, "no button to step into the room");
            }
            walk(&robot);
            save(&robot, &shots, "room.png");
            // The fire flies and the shape shifts until the motion stops,
            // and the app never goes idle while they move.
            if !click_button_in_semantics(&robot, "Stop the motion") {
                robot_exit::fail(&robot, "no button to stop the motion in the room");
            }
            robot_shot::settle(&robot, 300);
            click_shape_and_toggle(&robot);
            save(&robot, &shots, "toggle.png");

            let counter = button(&robot, "Pressed 0 times");
            // E turns the camera while it is held, Q turns it back.
            hold_key(&robot, "e");
            let turned = button(&robot, "Pressed 0 times");
            println!("  counter at {counter:?}, after E at {turned:?}");
            if (turned.0 - counter.0).abs() < 20.0 {
                robot_exit::fail(&robot, "holding E did not turn the camera");
            }
            hold_key(&robot, "q");

            if !click_button_in_semantics(&robot, "Pressed 0 times") {
                robot_exit::fail(&robot, "the far wall's counter is missing in the room");
            }
            robot_shot::settle(&robot, 300);
            if find_text_in_semantics(&robot, "Pressed 1 times").is_none() {
                robot_exit::fail(
                    &robot,
                    "a click on the far wall's counter, where it shows in perspective, missed it",
                );
            }

            // The left button stays with the content: its drag across the
            // floor leaves the camera where it is.
            let still = button(&robot, "Pressed 1 times");
            drag(&robot, PointerButton::Primary, (600.0, 760.0), (900.0, 760.0));
            let after_left = button(&robot, "Pressed 1 times");
            if (after_left.0 - still.0).abs() > 1.0 {
                robot_exit::fail(&robot, "a left drag across the floor turned the camera");
            }
            // The right button turns it, as a game's mouse look does.
            drag(&robot, PointerButton::Secondary, (600.0, 760.0), (700.0, 760.0));
            let after_right = button(&robot, "Pressed 1 times");
            println!("  counter at {still:?}, after a right drag at {after_right:?}");
            if after_right.2 <= 0.0 || (after_right.0 - still.0).abs() < 20.0 {
                robot_exit::fail(&robot, "a right drag across the floor did not turn the camera");
            }
            save(&robot, &shots, "turned.png");
            drag(&robot, PointerButton::Secondary, (700.0, 760.0), (600.0, 760.0));

            // The wall to the screen's right holds the whole demo again: its
            // Room tab opens a room inside the wall, flat until stepped into.
            click_wall_tab(&robot, "Room");
            if find_text_in_semantics(&robot, STEP_IN).is_none() {
                robot_exit::fail(&robot, "the demo on the wall did not open its room");
            }
            save(&robot, &shots, "nested.png");
            click_wall_tab(&robot, "Cranpose Guide");

            if !click_button_in_semantics(&robot, "Back to the screen") {
                robot_exit::fail(&robot, "no button to leave the room");
            }
            walk(&robot);
            robot_shot::settle(&robot, 300);
            save(&robot, &shots, "back.png");
            let back = step_in_button(&robot);
            if (back.0 - flat.0).abs() > 1.0 || (back.1 - flat.1).abs() > 1.0 {
                robot_exit::fail(
                    &robot,
                    &format!("the screen did not come back: its button moved from {flat:?} to {back:?}"),
                );
            }

            println!("PASS: room shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

/// Holds `key` for a tenth of a second.
fn hold_key(robot: &Robot, key: &str) {
    if let Err(err) = robot.send_key(key) {
        robot_exit::fail(robot, &format!("key {key}: {err}"));
    }
    std::thread::sleep(Duration::from_millis(100));
    if let Err(err) = robot.release_key(key) {
        robot_exit::fail(robot, &format!("releasing {key}: {err}"));
    }
    robot_shot::settle(robot, 100);
}

/// Lets the camera walk between the screen and the room.
fn walk(robot: &Robot) {
    std::thread::sleep(WALK);
    if let Err(err) = robot.pump_frames(3) {
        robot_exit::fail(robot, &format!("pump frames: {err}"));
    }
}

/// Clicks the button on the shape from a distance field, where it shows on
/// the shape, and the toggle on the far wall, which stands out of its card
/// as a volume, where its stage shows.
fn click_shape_and_toggle(robot: &Robot) {
    if !click_button_in_semantics(robot, "Clicked 0 times") {
        robot_exit::fail(robot, "the shape's button is missing");
    }
    robot_shot::settle(robot, 300);
    if find_text_in_semantics(robot, "Clicked 1 times").is_none() {
        robot_exit::fail(robot, "a click on the shape's button, where it shows, missed it");
    }
    let (before, (x, y, width, height)) = robot_controls_grid::card(robot, "Toggle");
    if let Err(err) = robot.click(x + width * 0.5, y + height * 0.5) {
        robot_exit::fail(robot, &format!("click the toggle: {err}"));
    }
    robot_shot::settle(robot, 300);
    let (after, _) = robot_controls_grid::card(robot, "Toggle");
    println!("  the far wall's toggle read {before}, then {after}");
    if (before.as_str(), after.as_str()) != ("Off", "On") {
        robot_exit::fail(
            robot,
            "a click on the toggle's stage, where it stands out of the wall, missed it",
        );
    }
}

/// Clicks the tab `label` of the demo on the east wall: of the labels that
/// read `label`, the one farthest right.
fn click_wall_tab(robot: &Robot, label: &str) {
    fn visit(element: &SemanticElement, label: &str, found: &mut Vec<(f32, f32, f32, f32)>) {
        if element.text.as_deref() == Some(label) {
            let bounds = element.bounds;
            found.push((bounds.x, bounds.y, bounds.width, bounds.height));
        }
        for child in &element.children {
            visit(child, label, found);
        }
    }
    let mut found = Vec::new();
    match robot.get_semantics() {
        Ok(roots) => roots.iter().for_each(|root| visit(root, label, &mut found)),
        Err(err) => robot_exit::fail(robot, &format!("semantics: {err}")),
    }
    let Some((x, y, width, height)) = found.into_iter().max_by(|a, b| a.0.total_cmp(&b.0)) else {
        robot_exit::fail(robot, &format!("no tab {label:?}"));
    };
    println!("  the wall's tab {label:?} at ({x:.1}, {y:.1})");
    if let Err(err) = robot.click(x + width * 0.5, y + height * 0.5) {
        robot_exit::fail(robot, &format!("click the wall's tab {label:?}: {err}"));
    }
    robot_shot::settle(robot, 500);
}

fn step_in_button(robot: &Robot) -> (f32, f32, f32, f32) {
    button(robot, STEP_IN)
}

fn button(robot: &Robot, label: &str) -> (f32, f32, f32, f32) {
    match robot.find_button_bounds_exact(label) {
        Ok(Some(bounds)) => bounds,
        Ok(None) => robot_exit::fail(robot, &format!("no button {label:?}")),
        Err(err) => robot_exit::fail(robot, &format!("button query: {err}")),
    }
}

/// Drags the mouse with `button` held from `from` to `to` in ten steps.
fn drag(robot: &Robot, button: PointerButton, from: (f32, f32), to: (f32, f32)) {
    let mut steps = (1..=10).map(|step| {
        let t = step as f32 / 10.0;
        (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
    });
    let dragged = robot
        .move_to(from.0, from.1)
        .and_then(|()| robot.mouse_button_down(button))
        .and_then(|()| steps.try_for_each(|(x, y)| robot.move_to(x, y)))
        .and_then(|()| robot.mouse_button_up(button));
    if let Err(err) = dragged {
        robot_exit::fail(robot, &format!("{button:?} drag across the floor: {err}"));
    }
    robot_shot::settle(robot, 400);
}

fn save(robot: &Robot, directory: &std::path::Path, name: &str) {
    match robot.screenshot() {
        Ok(shot) => robot_shot::save(&shot, directory, name),
        Err(err) => robot_exit::fail(robot, &format!("screenshot {name}: {err}")),
    }
}
