mod glass_backdrop_scroll_helpers;
mod output_paths;
mod robot_exit;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app;
use glass_backdrop_scroll_helpers::{GlassBackdropScrollRun, WINDOW_HEIGHT, WINDOW_WIDTH};
use text_showcase_external_helpers::{capture_x11_window_screenshot, find_window_id};

const WINDOW_TITLE: &str = "Robot Glass Backdrop Scroll Stability";

fn main() {
    env_logger::init();
    println!("=== Robot Glass Backdrop Scroll Stability ===");
    let output_dir = output_paths::diagnostic_path(&format!(
        "cranpose_glass_backdrop_scroll_stability-{}",
        std::process::id()
    ));
    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            let window_id = find_window_id(WINDOW_TITLE);
            let capture_path = output_dir.join("presented.png");
            let mut capture = |robot: &cranpose::Robot| {
                let started = std::time::Instant::now();
                let result = robot.wait_for_present_frame();
                println!(
                    "wait_for_present_frame took {:?} result={result:?}",
                    started.elapsed()
                );
                capture_x11_window_screenshot(
                    &window_id,
                    &capture_path,
                    WINDOW_WIDTH as f32,
                    WINDOW_HEIGHT as f32,
                )
            };
            GlassBackdropScrollRun {
                robot: &robot,
                output_dir: output_dir.clone(),
                capture: &mut capture,
            }
            .run();
        })
        .run(app::combined_app);
}
