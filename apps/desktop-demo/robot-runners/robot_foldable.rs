mod named_semantics;
mod robot_exit;

use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::changed_pixel_count_in_region;
use desktop_app::app;
use named_semantics::{expect_reading, named_control, Bounds};

/// The accessibility name the demo gives its drag surface.
const SCREEN: &str = "Foldable screen";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 720;
/// How much of the strip the folding half covers when the device is flat has
/// to be given up once it is most of the way shut. A half that never folded
/// goes on covering all of it.
const MIN_GIVEN_UP_PIXELS: usize = 8000;

/// The drag surface, as `(reading, bounds)`.
fn screen(robot: &Robot) -> (String, Bounds) {
    named_control(robot, SCREEN)
}

fn settle(robot: &Robot) {
    std::thread::sleep(Duration::from_millis(700));
    let _ = robot.wait_for_idle();
}

/// Drag right to left across the spread, in steps, and hold at the end.
fn drag_across(robot: &Robot, bounds: Bounds, fraction: f32) {
    let (x, y, width, height) = bounds;
    let start = x + width * 0.72;
    let stop = start - width * 0.32 * fraction;
    let mid_y = y + height * 0.5;
    robot.mouse_move(start, mid_y).expect("reach the screen");
    robot.mouse_down().expect("take the panel");
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
        .with_title("Foldable Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            settle(&robot);
            expect_reading(&robot, SCREEN, "Open", "the device starts flat open");

            let (_, bounds) = screen(&robot);
            let flat = robot.screenshot().expect("flat screenshot");

            drag_across(&robot, bounds, 0.8);
            let folding = robot.screenshot().expect("folding screenshot");
            let reading = screen(&robot).0;
            if !reading.ends_with("% folded") {
                robot_exit::fail_without_shutdown(&format!(
                    "a long drag left the device reading '{reading}', not part way folded"
                ));
            }

            // The strip the folding half covers when the device is flat. As it
            // folds, its picture is pressed into the crease and it gives that
            // strip up; nothing else on the stage reaches into it.
            let (x, y, width, height) = bounds;
            let strip = (
                x + width * 0.20,
                y + height * 0.30,
                width * 0.11,
                height * 0.40,
            );
            let given_up = changed_pixel_count_in_region(&flat, &folding, strip, 6);
            println!("given_up_pixels={given_up}");
            if given_up < MIN_GIVEN_UP_PIXELS {
                robot_exit::fail_without_shutdown(&format!(
                    "a device most of the way shut gave up {given_up} pixels of the strip its \
                     folding half covers when flat: that half never folded"
                ));
            }

            robot.mouse_up().expect("let the panel go");
            settle(&robot);
            expect_reading(
                &robot,
                SCREEN,
                "Shut",
                "a panel released past halfway folds the rest of the way",
            );

            drag_across(&robot, bounds, -0.9);
            robot.mouse_up().expect("let the panel go");
            settle(&robot);
            expect_reading(
                &robot,
                SCREEN,
                "Open",
                "dragging the other way opens the device again",
            );

            println!(
                "PASS: the folding half is pressed into the crease, gives up the width it held \
                 and settles open or shut"
            );
            robot.exit().expect("exit");
        })
        .run(app::FoldableRobotApp);
}
