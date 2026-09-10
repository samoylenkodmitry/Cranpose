mod named_semantics;
mod robot_exit;

use std::time::Duration;

use cranpose::{AppLauncher, Robot, RobotScreenshot};
use desktop_app::app;
use named_semantics::{expect_reading, named_control, Bounds};

/// The accessibility name the demo gives its drag surface.
const SCREEN: &str = "Foldable screen";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 720;
/// How much of the width it covers when flat the device has to give up once
/// it is most of the way shut. A device whose halves never turned keeps all
/// of it, whatever else happens to its picture.
const MOST_WIDTH_LEFT: f32 = 0.80;

/// The drag surface, as `(reading, bounds)`.
fn screen(robot: &Robot) -> (String, Bounds) {
    named_control(robot, SCREEN)
}

/// The colour of the stage the device stands on, read from a corner of the
/// drag surface the device never reaches.
fn stage_colour(shot: &RobotScreenshot, bounds: Bounds) -> [u8; 3] {
    let (x, y, _, height) = bounds;
    pixel_at(shot, x + 10.0, y + height * 0.5).unwrap_or([0, 0, 0])
}

fn pixel_at(shot: &RobotScreenshot, x: f32, y: f32) -> Option<[u8; 3]> {
    let scale = shot.width as f32 / shot.logical_width.max(1.0);
    let px = (x * scale).round() as usize;
    let py = (y * scale).round() as usize;
    if px >= shot.width as usize || py >= shot.height as usize {
        return None;
    }
    let at = (py * shot.width as usize + px) * 4;
    shot.pixels
        .get(at..at + 3)
        .map(|rgb| [rgb[0], rgb[1], rgb[2]])
}

/// How wide the device on the left of the stage is, measured along a line
/// through its middle: from the first thing on that line that is not the
/// stage to the last.
fn device_width(shot: &RobotScreenshot, bounds: Bounds, stage: [u8; 3]) -> f32 {
    let (x, y, width, height) = bounds;
    let middle = y + height * 0.5;
    let mut left = None;
    let mut right = x;
    let mut at = x + 2.0;
    while at < x + width * 0.5 {
        if let Some(pixel) = pixel_at(shot, at, middle) {
            let apart = (0..3)
                .map(|i| pixel[i].abs_diff(stage[i]) as u32)
                .sum::<u32>();
            if apart > 24 {
                if left.is_none() {
                    left = Some(at);
                }
                right = at;
            }
        }
        at += 1.0;
    }
    left.map(|first| right - first).unwrap_or(0.0)
}

fn settle(robot: &Robot) {
    std::thread::sleep(Duration::from_millis(700));
    let _ = robot.wait_for_idle();
}

/// Drag right to left across the spread, in steps, and hold at the end.
fn drag_across(robot: &Robot, bounds: Bounds, fraction: f32) {
    let (x, y, width, height) = bounds;
    let start = x + width * 0.38;
    let stop = start - width * 0.22 * fraction;
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

            // A device that folds gives up width: the half that swings away
            // from the reader covers less and less of the stage it stands on.
            let stage = stage_colour(&flat, bounds);
            let open_width = device_width(&flat, bounds, stage);
            let folded_width = device_width(&folding, bounds, stage);
            println!("open_width={open_width} folded_width={folded_width}");
            if open_width < bounds.2 * 0.3 {
                robot_exit::fail_without_shutdown(&format!(
                    "the device measured {open_width} wide with nothing folded: the line the \
                     measurement runs along is not across it"
                ));
            }
            if folded_width > open_width * MOST_WIDTH_LEFT {
                robot_exit::fail_without_shutdown(&format!(
                    "a device most of the way shut still measured {folded_width} wide against \
                     {open_width} flat open: neither half turned, whatever else happened to \
                     the picture"
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
                "PASS: the device gives up the width its halves cover when flat, and settles \
                 open or shut"
            );
            robot.exit().expect("exit");
        })
        .run(app::FoldableRobotApp);
}
