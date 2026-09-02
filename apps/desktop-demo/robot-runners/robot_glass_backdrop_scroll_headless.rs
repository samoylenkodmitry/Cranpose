mod glass_backdrop_scroll_helpers;
mod output_paths;
mod robot_exit;
mod robot_launch;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose_testing::capture_screenshot;
use desktop_app::app;
use glass_backdrop_scroll_helpers::{GlassBackdropScrollRun, WINDOW_HEIGHT, WINDOW_WIDTH};

fn main() {
    env_logger::init();
    println!("=== Robot Glass Backdrop Scroll (headless) ===");
    let output_dir = output_paths::diagnostic_path(&format!(
        "cranpose_glass_backdrop_scroll_headless-{}",
        std::process::id()
    ));
    robot_launch::launch("Robot Glass Backdrop Scroll", WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_test_driver(move |robot| {
            let mut capture = |robot: &cranpose::Robot| {
                capture_screenshot(robot)
                    .unwrap_or_else(|| robot_exit::fail(robot, "screenshot failed"))
            };
            GlassBackdropScrollRun {
                robot: &robot,
                output_dir,
                capture: &mut capture,
            }
            .run();
        })
        .run(app::combined_app);
}
