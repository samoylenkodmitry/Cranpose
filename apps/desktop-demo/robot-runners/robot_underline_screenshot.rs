//! Robot test: take REAL system screenshots of the underlined text at different scroll positions.
//!
//! NOT headless. NOT renderer screenshots. Actual X11 window capture via ImageMagick `import`.
//! Saves PNGs to /tmp/cranpose_underline_screenshots/ for human inspection.

mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use desktop_app::app;
use std::time::Duration;
use text_showcase_external_helpers::{
    find_window_id, open_text_tab, scroll_text_into_view, take_x11_screenshot,
};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const OUTPUT_DIR: &str = "/tmp/cranpose_underline_screenshots";
const WINDOW_TITLE: &str = "Robot Underline Screenshot";
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";

fn main() {
    env_logger::init();
    println!("=== Robot Underline Screenshot ===");
    println!("Output dir: {OUTPUT_DIR}");

    std::fs::create_dir_all(OUTPUT_DIR).expect("create output dir");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
            scroll_text_into_view(&robot, TARGET_TEXT, WINDOW_HEIGHT, 20);
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(WINDOW_TITLE);
            println!("Window ID: {window_id}");

            // Take screenshots at different scroll positions
            for step in 0..10 {
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();

                let bounds = find_text_in_semantics(&robot, TARGET_TEXT)
                    .expect("target text must be visible");
                println!(
                    "  step {step}: text at y={:.2} (x={:.2}, w={:.2}, h={:.2})",
                    bounds.1, bounds.0, bounds.2, bounds.3
                );

                // Full window screenshot
                let path = format!("{OUTPUT_DIR}/step_{step:02}_full.png");
                take_x11_screenshot(&window_id, &path);

                // Scroll by a small amount
                let center_x = bounds.0 + bounds.2 * 0.5;
                let center_y = bounds.1 + bounds.3 * 0.5;
                robot.mouse_move(center_x, center_y).expect("move cursor");
                std::thread::sleep(Duration::from_millis(50));
                robot.mouse_scroll(0.0, -0.7).expect("scroll");
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();
            }

            println!("\n=== Done ===");
            println!("Screenshots saved to {OUTPUT_DIR}/");
            println!("Compare them visually — underline thickness should be identical across all.");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}
