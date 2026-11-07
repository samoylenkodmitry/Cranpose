// The Backrooms tab: the first room shows in its yellow under the
// camcorder's REC with the entity's welcome on its wall, holding W walks
// the eye through the doorway ahead into the second room, and a drag turns
// the eye.

use std::time::Duration;

use cranpose::{PointerButton, Robot, RobotScreenshot};
use cranpose_testing::{find_text_in_semantics, find_text_in_semantics_exact};

use crate::{output_paths, pointer_drag, robot_exit, robot_launch, robot_shot};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
// Where the demo's tab bar ends, in logical pixels.
const TAB_BAR_BOTTOM: f32 = 120.0;
// The side of the squares whose mean colours the pictures are compared by,
// in logical pixels: wide enough that the grain averages out.
const BLOCK: f32 = 20.0;
// How much more than two still pictures a turn changes the picture, at
// least.
const MOVED_OVER_STILL: f32 = 3.0;
// How many tenths of a second W may be held for the eye to walk into the
// next room, 750 units ahead at 260 a second: a busy host draws fewer,
// longer frames, and the eye walks a tenth of a second a frame at most.
const LONGEST_HOLD_TENTHS: u32 = 150;

pub(crate) fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Backrooms Walk ===");
    robot_exit::arm_timeout(90);
    let shots = output_paths::diagnostic_path("cranpose-robot-backrooms");
    if let Err(err) = std::fs::create_dir_all(&shots) {
        robot_exit::fail_without_shutdown(&format!("{}: {err}", shots.display()));
    }

    robot_launch::launch("Robot Backrooms Walk", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_robot_app_hook(robot_launch::set_tab_hook)
        .with_test_driver(move |robot| {
            robot_launch::switch_tab(&robot, "backrooms");
            pause(&robot, 1500);
            for text in ["REC", "ROOM 1"] {
                if find_text_in_semantics_exact(&robot, text).is_none() {
                    robot_exit::fail(&robot, &format!("the camcorder shows no {text}"));
                }
            }
            if find_text_in_semantics(&robot, "WELCOME HOME").is_none() {
                robot_exit::fail(&robot, "the entity wrote no welcome on the wall");
            }
            let start = shot(&robot, &shots, "start.png");
            let (red, green, blue) = mean_colour(&start);
            println!("  mean colour {red:.0} {green:.0} {blue:.0}");
            if red < blue + 25.0 || green < blue + 15.0 {
                robot_exit::fail(&robot, "the room does not show in its yellow");
            }
            pause(&robot, 500);
            let still = difference(&start, &shot(&robot, &shots, "still.png"));
            let moved = still * MOVED_OVER_STILL + 2.0;

            hold_until_next_room(&robot, "w");
            let walked = shot(&robot, &shots, "walked.png");

            pointer_drag::drag(&robot, PointerButton::Primary, (500.0, 450.0), (800.0, 450.0));
            pause(&robot, 300);
            let turn = difference(&walked, &shot(&robot, &shots, "turned.png"));
            println!("  turned {turn:.2}");
            if turn < moved {
                robot_exit::fail(&robot, "a drag did not turn the eye");
            }

            println!("PASS: backrooms shots in {}", shots.display());
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}

// Lets `millis` pass with frames drawn: the camcorder's clock runs on, so
// the app is never idle.
fn pause(robot: &Robot, millis: u64) {
    std::thread::sleep(Duration::from_millis(millis));
    if let Err(err) = robot.pump_frames(3) {
        robot_exit::fail(robot, &format!("pump frames: {err}"));
    }
}

// Holds `key` until the camcorder shows the eye in its second room.
fn hold_until_next_room(robot: &Robot, key: &str) {
    if let Err(err) = robot.send_key(key) {
        robot_exit::fail(robot, &format!("key {key}: {err}"));
    }
    let mut tenths = 0;
    while find_text_in_semantics_exact(robot, "ROOM 2").is_none() {
        pause(robot, 100);
        tenths += 1;
        if tenths >= LONGEST_HOLD_TENTHS {
            robot_exit::fail(robot, &format!("holding {key} did not walk into the next room"));
        }
    }
    println!("  holding {key} walked into the next room in {tenths} tenths");
    if let Err(err) = robot.release_key(key) {
        robot_exit::fail(robot, &format!("releasing {key}: {err}"));
    }
}

fn shot(robot: &Robot, directory: &std::path::Path, name: &str) -> RobotScreenshot {
    match robot.screenshot() {
        Ok(shot) => {
            robot_shot::save(&shot, directory, name);
            shot
        }
        Err(err) => robot_exit::fail(robot, &format!("screenshot {name}: {err}")),
    }
}

// The mean colour of each block of the picture below the tab bar.
fn blocks(shot: &RobotScreenshot) -> Vec<(f32, f32, f32)> {
    let sample = robot_shot::logical_sampler(shot);
    let columns = (shot.logical_width / BLOCK) as u32;
    let rows = ((shot.logical_height - TAB_BAR_BOTTOM) / BLOCK) as u32;
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (row, column)))
        .map(|(row, column)| {
            let mut sum = (0.0, 0.0, 0.0);
            for step in 0..16 {
                let x = (column as f32 + (step % 4) as f32 / 4.0 + 0.125) * BLOCK;
                let y = TAB_BAR_BOTTOM + (row as f32 + (step / 4) as f32 / 4.0 + 0.125) * BLOCK;
                let (r, g, b) = sample(x, y);
                sum = (sum.0 + r as f32, sum.1 + g as f32, sum.2 + b as f32);
            }
            (sum.0 / 16.0, sum.1 / 16.0, sum.2 / 16.0)
        })
        .collect()
}

fn mean_colour(shot: &RobotScreenshot) -> (f32, f32, f32) {
    let blocks = blocks(shot);
    let count = blocks.len().max(1) as f32;
    let sum = blocks.iter().fold((0.0, 0.0, 0.0), |sum, block| {
        (sum.0 + block.0, sum.1 + block.1, sum.2 + block.2)
    });
    (sum.0 / count, sum.1 / count, sum.2 / count)
}

// How far the blocks of two pictures differ in colour, on the mean.
fn difference(a: &RobotScreenshot, b: &RobotScreenshot) -> f32 {
    let (a, b) = (blocks(a), blocks(b));
    let count = a.len().max(1) as f32;
    a.iter()
        .zip(&b)
        .map(|(a, b)| ((a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()) / 3.0)
        .sum::<f32>()
        / count
}
