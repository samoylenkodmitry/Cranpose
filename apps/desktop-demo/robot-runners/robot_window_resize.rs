// A window resized again and again takes each size asked of it.

use std::time::{Duration, Instant};

use crate::robot_launch;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
// The sizes the window takes in turn, as steps from the size it opens at,
// in physical pixels; the last step brings it back.
const STEPS: [(i32, i32); 5] = [(96, 64), (-96, -64), (160, 96), (-64, -32), (0, 0)];
// The time a resize may take to reach the surface: a loaded X server
// configures the window long after the frames of an unpaced display.
const RESIZE_TIME: Duration = Duration::from_secs(5);

pub(crate) fn main() {
    println!("=== Robot Window Resize Test ===");
    robot_launch::launch("Robot Window Resize", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("the demo settles");
            let (width, height) = robot
                .presentation_info()
                .expect("presentation info")
                .surface_size;
            for (dx, dy) in STEPS {
                let size = (
                    width.saturating_add_signed(dx),
                    height.saturating_add_signed(dy),
                );
                robot
                    .request_window_size(size.0, size.1)
                    .expect("the window takes the size request");
                let asked = Instant::now();
                let mut shown = (0, 0);
                while shown != size && asked.elapsed() < RESIZE_TIME {
                    robot.pump_frames(1).expect("a frame at the new size");
                    shown = robot
                        .presentation_info()
                        .expect("presentation info")
                        .surface_size;
                }
                println!("asked for {size:?}, the surface is {shown:?}");
                if shown != size {
                    println!("FAIL: the window did not take the size {size:?}");
                    std::process::exit(1);
                }
            }
            println!("PASS: the window took each size");
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}
