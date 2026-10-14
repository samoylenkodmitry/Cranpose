//! The app opens another GPU device when its device is lost: frames reach
//! the screen again, and the composition keeps its state and its input.
//! The device is lost twice in a row, each time after a frame.

use std::time::Duration;

use cranpose::Robot;
use cranpose_core::{mutableStateOf, remember};
use cranpose_testing::{find_in_semantics, find_text};
use cranpose_ui::{
    Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Text, TextStyle,
};

use crate::{robot_exit, robot_launch, robot_pixels, text_input_robot_helpers};

const RED: [u8; 4] = [220, 40, 40, 255];

/// Whether the red box shows in a fresh frame.
fn red_box_shows(robot: &Robot) -> bool {
    let _ = robot.wait_for_idle();
    robot.screenshot().is_ok_and(|shot| {
        robot_pixels::color_close(robot_pixels::pixel_at_logical(&shot, 50.0, 30.0), RED, 8)
    })
}

/// Clicks the counter and waits for it to read `count`.
fn count_to(robot: &Robot, count: u32) -> bool {
    let label = format!("Count {}", count - 1);
    let Some((x, y, w, h)) = find_in_semantics(robot, |element| find_text(element, &label))
    else {
        return false;
    };
    let _ = robot.click(x + w / 2.0, y + h / 2.0);
    let expected = format!("Count {count}");
    text_input_robot_helpers::wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |element| find_text(element, &expected))
    })
    .is_some()
}

pub(crate) fn main() {
    env_logger::init();
    robot_launch::launch("GPU Device Loss", 300, 200)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            if !red_box_shows(&robot) {
                robot_exit::fail(&robot, "the first frame did not show the red box");
            }
            if !count_to(&robot, 1) {
                robot_exit::fail(&robot, "the counter did not count before the loss");
            }
            for (loss, count) in [(1, 2), (2, 3)] {
                if let Err(error) = robot.lose_gpu_device() {
                    robot_exit::fail(&robot, &format!("losing the device failed: {error}"));
                }
                std::thread::sleep(Duration::from_millis(200));
                if !red_box_shows(&robot) {
                    robot_exit::fail(&robot, &format!("no frame after loss {loss}"));
                }
                if !count_to(&robot, count) {
                    robot_exit::fail(&robot, &format!("the counter lost its state or input after loss {loss}"));
                }
            }
            println!("PASS: the app drew and counted on through two device losses");
            let _ = robot.exit();
        })
        .run(|| {
            let count = remember(|| mutableStateOf(0u32)).with(|count| *count);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Box(
                    Modifier::empty()
                        .size_points(100.0, 60.0)
                        .background(Color::from_rgb_u8(RED[0], RED[1], RED[2])),
                    BoxSpec::default(),
                    || {},
                );
                Text(
                    format!("Count {}", count.get()),
                    Modifier::empty()
                        .padding(8.0)
                        .clickable(move |_| count.set(count.get() + 1)),
                    TextStyle::default(),
                );
            });
        });
}
