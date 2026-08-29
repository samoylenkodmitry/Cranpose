use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::capture_screenshot;
use desktop_app::test_screens::pressed_state_repro::{
    PressedStateReproScreen, NORMAL_RGBA, PRESSED_RGBA, SPRITE_HEIGHT, SPRITE_OFFSET_X,
    SPRITE_OFFSET_Y, SPRITE_WIDTH,
};

const COLOR_TOLERANCE: i32 = 24;
const SAMPLE_ATTEMPTS: usize = 20;

fn fail(robot: &Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn physical_scale(screenshot: &cranpose::RobotScreenshot) -> f32 {
    if screenshot.logical_width.is_finite() && screenshot.logical_width > 0.0 {
        screenshot.width as f32 / screenshot.logical_width
    } else {
        1.0
    }
}

fn pixel_at_logical(screenshot: &cranpose::RobotScreenshot, x: f32, y: f32) -> [u8; 4] {
    let scale = physical_scale(screenshot);
    let px = ((x * scale) as u32).min(screenshot.width.saturating_sub(1));
    let py = ((y * scale) as u32).min(screenshot.height.saturating_sub(1));
    let index = (py as usize * screenshot.width as usize + px as usize) * 4;
    let bytes = &screenshot.pixels[index..index + 4];
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

fn color_close(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual
        .iter()
        .zip(expected.iter())
        .take(3)
        .all(|(a, e)| (*a as i32 - *e as i32).abs() <= COLOR_TOLERANCE)
}

fn wait_for_sprite_color(robot: &Robot, x: f32, y: f32, expected: [u8; 4]) -> Result<(), [u8; 4]> {
    let mut last = [0u8; 4];
    for _ in 0..SAMPLE_ATTEMPTS {
        std::thread::sleep(Duration::from_millis(50));
        let _ = robot.wait_for_idle();
        if let Some(screenshot) = capture_screenshot(robot) {
            last = pixel_at_logical(&screenshot, x, y);
            if color_close(last, expected) {
                return Ok(());
            }
        }
    }
    Err(last)
}

fn main() {
    env_logger::init();
    println!("=== Robot Pressed State Test ===");

    AppLauncher::new()
        .with_title("Robot Pressed State Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let center_x = SPRITE_OFFSET_X + SPRITE_WIDTH * 0.5;
            let center_y = SPRITE_OFFSET_Y + SPRITE_HEIGHT * 0.5;

            if let Err(actual) = wait_for_sprite_color(&robot, center_x, center_y, NORMAL_RGBA) {
                fail(
                    &robot,
                    &format!("sprite should start with the normal color, got {actual:?}"),
                );
            }
            println!("Baseline normal sprite confirmed");

            robot.mouse_move(center_x, center_y).expect("mouse move");
            std::thread::sleep(Duration::from_millis(50));
            robot.mouse_down().expect("mouse down");

            if let Err(actual) = wait_for_sprite_color(&robot, center_x, center_y, PRESSED_RGBA) {
                let _ = robot.mouse_up();
                fail(
                    &robot,
                    &format!(
                        "sprite must show the pressed color while the pointer is down, got {actual:?}"
                    ),
                );
            }
            println!("Pressed sprite confirmed while pointer is down");

            robot.mouse_up().expect("mouse up");

            if let Err(actual) = wait_for_sprite_color(&robot, center_x, center_y, NORMAL_RGBA) {
                fail(
                    &robot,
                    &format!("sprite should return to the normal color after release, got {actual:?}"),
                );
            }
            println!("Normal sprite restored after release");

            println!("✓ PASS: pressed sprite is drawn while the pointer is down");
            let _ = robot.exit();
        })
        .run(|| {
            PressedStateReproScreen();
        });
}
