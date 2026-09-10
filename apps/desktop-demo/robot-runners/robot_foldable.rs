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
/// How much of the strip the folding half covers when the device is flat has
/// to read as the stage behind it once the device is most of the way shut.
/// A half that dimmed but never folded goes on covering all of it.
const MIN_VACATED_PERCENT: usize = 75;
/// How little of that strip may read as the stage while the device is flat.
/// Any more and the strip was never over the folding half to begin with.
const MAX_FLAT_STAGE_PERCENT: usize = 8;

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

/// How much of `region`, per cent, reads as the stage rather than as the
/// device standing on it.
fn stage_percent(shot: &RobotScreenshot, region: Bounds, stage: [u8; 3]) -> usize {
    let (x, y, width, height) = region;
    let step = 2.0;
    let mut seen = 0usize;
    let mut stage_pixels = 0usize;
    let mut at_y = y;
    while at_y < y + height {
        let mut at_x = x;
        while at_x < x + width {
            if let Some(pixel) = pixel_at(shot, at_x, at_y) {
                seen += 1;
                let apart = (0..3)
                    .map(|i| pixel[i].abs_diff(stage[i]) as u32)
                    .sum::<u32>();
                if apart <= 24 {
                    stage_pixels += 1;
                }
            }
            at_x += step;
        }
        at_y += step;
    }
    if seen == 0 {
        return 0;
    }
    stage_pixels * 100 / seen
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

            // The strip the folding half covers when the device is flat. As it
            // folds it turns off square and covers less and less of that
            // strip, until the stage behind the device shows through it.
            let (x, y, width, height) = bounds;
            let strip = (
                x + width * 0.05,
                y + height * 0.30,
                width * 0.06,
                height * 0.40,
            );
            let stage = stage_colour(&flat, bounds);
            let flat_stage = stage_percent(&flat, strip, stage);
            let folded_stage = stage_percent(&folding, strip, stage);
            println!("flat_stage={flat_stage}% folded_stage={folded_stage}%");
            if flat_stage > MAX_FLAT_STAGE_PERCENT {
                robot_exit::fail_without_shutdown(&format!(
                    "the strip reads {flat_stage}% stage with the device flat open: it is not \
                     over the folding half"
                ));
            }
            if folded_stage < MIN_VACATED_PERCENT {
                robot_exit::fail_without_shutdown(&format!(
                    "a device most of the way shut left {folded_stage}% of the strip its folding \
                     half covers when flat reading as stage: that half never turned, whatever \
                     else happened to its picture"
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
                "PASS: the folding half turns off square, gives up the width it covered and \
                 settles open or shut"
            );
            robot.exit().expect("exit");
        })
        .run(app::FoldableRobotApp);
}
