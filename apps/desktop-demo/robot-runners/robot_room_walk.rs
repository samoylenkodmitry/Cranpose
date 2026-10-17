use std::time::Duration;

use crate::{output_paths, robot_exit, robot_launch, robot_shot};

use cranpose::Robot;
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
            robot_shot::settle(&robot, 300);
            let flat = step_in_button(&robot);
            save(&robot, &shots, "flat.png");

            if !click_button_in_semantics(&robot, STEP_IN) {
                robot_exit::fail(&robot, "no button to step into the room");
            }
            // The globe spins, so the app does not go idle until it stops.
            walk(&robot);
            save(&robot, &shots, "room.png");

            if !click_button_in_semantics(&robot, "Stop the spin") {
                robot_exit::fail(&robot, "the globe's button is missing in the room");
            }
            robot_shot::settle(&robot, 300);
            if find_text_in_semantics(&robot, "Spin the globe").is_none() {
                robot_exit::fail(
                    &robot,
                    "a click on the globe's button, where it shows on the sphere, missed it",
                );
            }

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

            drag(&robot, (600.0, 760.0), (900.0, 760.0));
            save(&robot, &shots, "turned.png");
            drag(&robot, (900.0, 760.0), (600.0, 760.0));

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

/// Lets the camera walk between the screen and the room.
fn walk(robot: &Robot) {
    std::thread::sleep(WALK);
    if let Err(err) = robot.pump_frames(3) {
        robot_exit::fail(robot, &format!("pump frames: {err}"));
    }
}

fn step_in_button(robot: &Robot) -> (f32, f32, f32, f32) {
    match robot.find_button_bounds_exact(STEP_IN) {
        Ok(Some(bounds)) => bounds,
        Ok(None) => robot_exit::fail(robot, "the flat screen shows no button to step in"),
        Err(err) => robot_exit::fail(robot, &format!("button query: {err}")),
    }
}

fn drag(robot: &Robot, from: (f32, f32), to: (f32, f32)) {
    if let Err(err) = robot.drag(from.0, from.1, to.0, to.1) {
        robot_exit::fail(robot, &format!("drag across the floor: {err}"));
    }
    robot_shot::settle(robot, 400);
}

fn save(robot: &Robot, directory: &std::path::Path, name: &str) {
    match robot.screenshot() {
        Ok(shot) => robot_shot::save(&shot, directory, name),
        Err(err) => robot_exit::fail(robot, &format!("screenshot {name}: {err}")),
    }
}
