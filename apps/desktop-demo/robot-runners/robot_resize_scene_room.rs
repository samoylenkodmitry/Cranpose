// A window resized again and again keeps the scene room of the one whole
// frame it kept before: each frame after a resize draws whole, and a whole
// frame for images that hold nothing goes to the present thread alone, so
// two whole scenes never live at once.

use crate::robot_launch;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
// The sizes the window takes in turn, as steps from the size it opens at,
// in physical pixels; the last step brings it back.
const STEPS: [(i32, i32); 5] = [(96, 64), (-96, -64), (160, 96), (-64, -32), (0, 0)];
// The frames a resize may take to reach the surface.
const RESIZE_FRAMES: usize = 60;

pub(crate) fn main() {
    println!("=== Robot Resize Scene Room Test ===");
    robot_launch::launch("Robot Resize Scene Room", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("the demo settles");
            let (width, height) = robot
                .presentation_info()
                .expect("presentation info")
                .surface_size;
            let before = robot
                .get_render_cpu_allocation_stats()
                .expect("stats before the resizes")
                .scene_room_bytes;
            for (dx, dy) in STEPS {
                let size = (
                    width.saturating_add_signed(dx),
                    height.saturating_add_signed(dy),
                );
                robot
                    .request_window_size(size.0, size.1)
                    .expect("the window takes the size request");
                let mut shown = (0, 0);
                for _ in 0..RESIZE_FRAMES {
                    robot.pump_frames(1).expect("a frame at the new size");
                    shown = robot
                        .presentation_info()
                        .expect("presentation info")
                        .surface_size;
                    if shown == size {
                        break;
                    }
                }
                println!("asked for {size:?}, the surface is {shown:?}");
                if shown != size {
                    println!("FAIL: the window did not take the size {size:?}");
                    std::process::exit(1);
                }
            }
            robot.wait_for_idle().expect("the demo settles after the resizes");
            let after = robot
                .get_render_cpu_allocation_stats()
                .expect("stats after the resizes")
                .scene_room_bytes;
            println!("scene room before {before} bytes, after the resizes {after} bytes");
            // A second whole scene would double the room; a frame of the
            // larger sizes may leave its scene a little roomier.
            if after > before + before / 4 {
                println!(
                    "FAIL: the resizes left {after} bytes of scene room, more than 1.25 times the \
                     {before} of one whole frame: two whole scenes lived at once"
                );
                std::process::exit(1);
            }
            println!("PASS: the resizes kept the scene room of one whole frame");
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}
