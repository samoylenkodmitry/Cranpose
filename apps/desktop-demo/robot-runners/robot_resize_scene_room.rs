// A window resized again and again keeps the scene room of the one whole
// frame it kept before: each frame after a resize draws whole, and a whole
// frame for images that hold nothing goes to the present thread alone, so
// two whole scenes never live at once. The window shows, as a hidden one
// draws no frame and keeps no scene to read.

use crate::robot_launch;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
// The sizes the window takes in turn, as steps from the size it opens at,
// in physical pixels; the last step brings it back.
const STEPS: [(i32, i32); 5] = [(96, 64), (-96, -64), (160, 96), (-64, -32), (0, 0)];
// The frames a resize may take to reach the surface.
const RESIZE_FRAMES: usize = 60;
// The frames after a resize reached the surface: the room reads none while
// the scene of the frame at the new size is out with that frame.
const SETTLE_FRAMES: usize = 4;

pub(crate) fn main() {
    println!("=== Robot Resize Scene Room Test ===");
    robot_launch::launch("Robot Resize Scene Room", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("the demo settles");
            let (width, height) = robot
                .presentation_info()
                .expect("presentation info")
                .surface_size;
            // The room right after each resize, read before the shell rests
            // and trims it away: the first resize's is that of one whole
            // frame.
            let mut rooms = Vec::with_capacity(STEPS.len());
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
                let mut room = 0;
                for _ in 0..SETTLE_FRAMES {
                    robot.pump_frames(1).expect("a frame at the new size");
                    let kept = robot
                        .get_render_cpu_allocation_stats()
                        .expect("stats after the resize")
                        .scene_room_bytes;
                    room = room.max(kept);
                }
                println!("scene room after the resize to {size:?}: {room} bytes");
                rooms.push(room);
            }
            let first = rooms[0];
            let most = rooms.iter().copied().max().unwrap_or(0);
            if first == 0 {
                println!(
                    "FAIL: the stats read no scene room after a resize: the check compares \
                     nothing"
                );
                std::process::exit(1);
            }
            // A second whole scene would double the room; a frame of the
            // larger sizes may leave its scene a little roomier.
            if most > first + first / 4 {
                println!(
                    "FAIL: a resize left {most} bytes of scene room, more than 1.25 times the \
                     {first} of one whole frame: two whole scenes lived at once"
                );
                std::process::exit(1);
            }
            println!("PASS: the resizes kept the scene room of one whole frame");
            robot.exit().ok();
        })
        .run(robot_launch::counter_demo);
}
