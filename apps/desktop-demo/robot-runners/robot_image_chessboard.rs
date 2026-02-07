//! Robot test validating image rendering and screenshot pixel assertions.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_image_chessboard --features robot-app`

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_in_semantics, find_text, screenshot_pixel};
use desktop_app::app;
use std::time::Duration;

const EXPECTED_LIGHT: [u8; 3] = [240, 240, 240];
const EXPECTED_DARK: [u8; 3] = [36, 54, 72];
const COLOR_TOLERANCE: u8 = 25;

fn within_tolerance(actual: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
    let tolerance = tolerance as i16;
    (actual[0] as i16 - expected[0] as i16).abs() <= tolerance
        && (actual[1] as i16 - expected[1] as i16).abs() <= tolerance
        && (actual[2] as i16 - expected[2] as i16).abs() <= tolerance
}

fn main() {
    env_logger::init();
    println!("=== Robot Image Chessboard Screenshot Test ===");

    AppLauncher::new()
        .with_title("Robot Image Chessboard")
        .with_size(1200, 800)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            let Some((tab_x, tab_y, tab_w, tab_h)) = find_button_in_semantics(&robot, "Images")
            else {
                println!("FATAL: Images tab not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            robot.click(tab_x + tab_w / 2.0, tab_y + tab_h / 2.0).ok();
            std::thread::sleep(Duration::from_millis(350));
            let _ = robot.wait_for_idle();

            let Some((img_x, img_y, img_w, img_h)) =
                find_in_semantics(&robot, |elem| find_text(elem, "Generated chessboard"))
            else {
                println!("FATAL: Chessboard semantics node not found");
                robot.exit().ok();
                std::process::exit(1);
            };

            if img_w < 80.0 || img_h < 80.0 {
                println!("FATAL: Chessboard bounds too small: ({img_x:.1}, {img_y:.1}, {img_w:.1}, {img_h:.1})");
                robot.exit().ok();
                std::process::exit(1);
            }

            let screenshot = match robot.screenshot() {
                Ok(image) => image,
                Err(err) => {
                    println!("FATAL: screenshot failed: {err}");
                    robot.exit().ok();
                    std::process::exit(1);
                }
            };

            let sample_cell = |cell_x: u32, cell_y: u32| -> Option<[u8; 3]> {
                let px = (img_x + ((cell_x as f32 + 0.5) * (img_w / 8.0))).floor() as u32;
                let py = (img_y + ((cell_y as f32 + 0.5) * (img_h / 8.0))).floor() as u32;
                let rgba = screenshot_pixel(&screenshot, px, py)?;
                Some([rgba[0], rgba[1], rgba[2]])
            };

            let checks = [
                (0, 0, EXPECTED_LIGHT),
                (1, 0, EXPECTED_DARK),
                (0, 1, EXPECTED_DARK),
                (1, 1, EXPECTED_LIGHT),
            ];

            for (cell_x, cell_y, expected) in checks {
                let Some(actual) = sample_cell(cell_x, cell_y) else {
                    println!("FATAL: failed to sample cell ({cell_x}, {cell_y})");
                    robot.exit().ok();
                    std::process::exit(1);
                };

                if !within_tolerance(actual, expected, COLOR_TOLERANCE) {
                    println!(
                        "FATAL: unexpected color at cell ({cell_x}, {cell_y}) actual={actual:?} expected={expected:?}"
                    );
                    robot.exit().ok();
                    std::process::exit(1);
                }
            }

            println!("PASS: screenshot pixel checks match chessboard pattern");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
