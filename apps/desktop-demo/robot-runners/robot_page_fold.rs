mod named_semantics;
mod robot_exit;

use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::changed_pixel_count_in_region;
use desktop_app::app;
use named_semantics::{expect_reading, named_control, Bounds};

/// The accessibility name the demo gives its drag surface.
const SPREAD: &str = "Book spread";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 700;
/// How much of the band above the spread a standing leaf has to cover. A leaf
/// that never left the page plane covers none of it.
const MIN_LIFTED_PIXELS: usize = 600;

/// The drag surface, as `(reading, bounds)`.
fn spread(robot: &Robot) -> (String, Bounds) {
    named_control(robot, SPREAD)
}

fn settle(robot: &Robot) {
    std::thread::sleep(Duration::from_millis(700));
    let _ = robot.wait_for_idle();
}

/// Drag right to left across the spread, in steps, and hold at the end.
fn drag_across(robot: &Robot, bounds: Bounds, fraction: f32) {
    let (x, y, width, height) = bounds;
    let start = x + width * 0.86;
    let stop = start - width * 0.62 * fraction;
    let mid_y = y + height * 0.5;
    robot.mouse_move(start, mid_y).expect("reach the spread");
    robot.mouse_down().expect("take the page");
    let steps = 6;
    for step in 1..=steps {
        let at = start + (stop - start) * step as f32 / steps as f32;
        robot.mouse_move(at, mid_y).expect("drag");
        std::thread::sleep(Duration::from_millis(40));
    }
    let _ = robot.wait_for_idle();
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Page Fold Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            settle(&robot);
            expect_reading(&robot, SPREAD, "Leaf 1 of 7, at rest", "the book opens at its first leaf");

            let (_, bounds) = spread(&robot);
            let flat = robot.screenshot().expect("flat screenshot");

            drag_across(&robot, bounds, 0.5);
            let lifted = robot.screenshot().expect("lifted screenshot");
            if !spread(&robot).0.ends_with("turning") {
                robot_exit::fail_without_shutdown("a half drag did not report a leaf in motion");
            }

            // The band along the top of the drag surface, above where the flat
            // pages reach. Only a leaf standing off the page plane reaches it.
            let (x, y, width, _) = bounds;
            let band = (x + width * 0.25, y + 4.0, width * 0.5, 40.0);
            let lifted_pixels = changed_pixel_count_in_region(&flat, &lifted, band, 6);
            println!("lifted_pixels={lifted_pixels}");
            if lifted_pixels < MIN_LIFTED_PIXELS {
                robot_exit::fail_without_shutdown(&format!(
                    "a half-turned leaf covered {lifted_pixels} pixels above the flat spread: it \
                     never left the page plane"
                ));
            }

            robot.mouse_up().expect("let the page go");
            settle(&robot);
            expect_reading(
                &robot,
                SPREAD,
                "Leaf 2 of 7, at rest",
                "a leaf released past halfway falls the rest of the way",
            );

            drag_across(&robot, bounds, 0.12);
            robot.mouse_up().expect("let the page go");
            settle(&robot);
            expect_reading(
                &robot,
                SPREAD,
                "Leaf 2 of 7, at rest",
                "a leaf released short of halfway falls back where it came from",
            );

            println!("PASS: the spread turns a leaf, lifts it off the page plane and springs a short drag back");
            robot.exit().expect("exit");
        })
        .run(app::PageFoldRobotApp);
}
