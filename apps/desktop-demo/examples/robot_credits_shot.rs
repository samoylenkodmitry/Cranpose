//! Captures the Credits (cranorbit) tab to a PNG.
//!
//! The tab draws cranorbit's own Credits screen, so this is what that screen
//! looks like rendered by the desktop backend rather than on a watch.

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use image::RgbaImage;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let _ = env_logger::try_init();
    let out = PathBuf::from(
        std::env::var("CRANPOSE_SHOT_OUT").unwrap_or_else(|_| "target/credits.png".to_string()),
    );
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("create shot dir");
    }

    AppLauncher::new()
        .with_title("Credits shot")
        .with_size(900, 700)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();
            let (x, y, w, h) = find_text_in_semantics(&robot, "Credits (cranorbit)")
                .expect("the Credits tab must be on screen");
            robot
                .click(x + w * 0.5, y + h * 0.5)
                .expect("click the Credits tab");
            let _ = robot.wait_for_idle();
            // Let the list scroll into a representative position rather than
            // catching it at its starting edge.
            std::thread::sleep(Duration::from_millis(1200));

            let shot = robot.screenshot().expect("screenshot");
            let image = RgbaImage::from_raw(shot.width, shot.height, shot.pixels)
                .expect("screenshot buffer size");
            image.save(&out).expect("write png");
            println!("captured -> {}", out.display());
            robot.exit().ok();
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");
}
