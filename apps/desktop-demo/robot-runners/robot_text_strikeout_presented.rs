//! External X11 contract for Text tab line-through rendering.

mod output_paths;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use cranpose_testing::crop_screenshot_logical;
use desktop_app::app;
use image::RgbaImage;
use std::path::Path;
use std::time::Duration;
use text_showcase_external_helpers::{
    capture_x11_window_screenshot, find_exact_text_in_semantics, find_window_id, open_text_tab,
    wait_for_text_showcase_heading,
};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const WINDOW_TITLE: &str = "Robot Text Strikeout Presented";
const TARGET_TEXT: &str = "Decorated shadow text";

#[derive(Clone, Copy, Debug)]
struct StrikeoutMetrics {
    row: u32,
    first_x: u32,
    last_x: u32,
    coverage: f32,
    max_dark_gap: u32,
}

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn main() {
    env_logger::init();
    println!("=== Robot Text Strikeout Presented ===");

    let output_dir = output_paths::diagnostic_path("cranpose_text_strikeout_presented");
    std::fs::create_dir_all(&output_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", output_dir.display()));
    println!("Output dir: {}", output_dir.display());

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(800));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
            robot
                .wait_for_present_frame()
                .expect("present Text tab before strikeout capture");
            wait_for_text_showcase_heading(&robot);
            let bounds = find_exact_text_in_semantics(&robot, TARGET_TEXT)
                .unwrap_or_else(|| fail(&robot, "strikeout text semantics bounds not found"));
            println!("Target bounds: {bounds:?}");

            let window_id = find_window_id(WINDOW_TITLE);
            println!("Window ID: {window_id}");

            let full_path = output_dir.join("full.png");
            let full = capture_x11_window_screenshot(
                &window_id,
                &full_path,
                WINDOW_WIDTH as f32,
                WINDOW_HEIGHT as f32,
            );

            let crop_padding = 12.0;
            let crop = crop_screenshot_logical(
                &full,
                bounds.0 - crop_padding,
                bounds.1 - crop_padding,
                bounds.2 + crop_padding * 2.0,
                bounds.3 + crop_padding * 2.0,
            )
            .unwrap_or_else(|| fail(&robot, "failed to crop strikeout text from X11 capture"));
            let crop_path = output_dir.join("strikeout_crop.png");
            save_robot_screenshot(&crop, &crop_path);

            let metrics = strikeout_metrics(&crop)
                .unwrap_or_else(|| fail(&robot, "could not locate line-through row in crop"));
            println!(
                "Strikeout metrics: row={} first_x={} last_x={} coverage={:.3} max_dark_gap={}",
                metrics.row, metrics.first_x, metrics.last_x, metrics.coverage, metrics.max_dark_gap
            );

            if metrics.coverage < 0.86 || metrics.max_dark_gap > 2 {
                fail(
                    &robot,
                    &format!(
                        "line-through must be continuous in the presented X11 capture; coverage={:.3}, max_dark_gap={}",
                        metrics.coverage, metrics.max_dark_gap
                    ),
                );
            }

            println!("PASS: Text tab line-through is continuous in presented X11 capture");
            robot.exit().ok();
        })
        .run(|| app::combined_app_with_initial_tab(Some(app::DemoTab::Text)));
}

fn save_robot_screenshot(screenshot: &cranpose::RobotScreenshot, path: &Path) {
    let image = RgbaImage::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .unwrap_or_else(|| panic!("invalid screenshot buffer for {}", path.display()));
    image
        .save(path)
        .unwrap_or_else(|err| panic!("failed to save {}: {err}", path.display()));
}

fn strikeout_metrics(screenshot: &cranpose::RobotScreenshot) -> Option<StrikeoutMetrics> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let start_y = (height as f32 * 0.25).floor() as usize;
    let end_y = (height as f32 * 0.70).ceil() as usize;
    (start_y..end_y.min(height))
        .filter_map(|row| strikeout_row_metrics(screenshot, row))
        .max_by(|a, b| {
            a.coverage
                .total_cmp(&b.coverage)
                .then_with(|| b.max_dark_gap.cmp(&a.max_dark_gap))
        })
}

fn strikeout_row_metrics(
    screenshot: &cranpose::RobotScreenshot,
    row: usize,
) -> Option<StrikeoutMetrics> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut bright = vec![false; width];
    for (x, bright_slot) in bright.iter_mut().enumerate() {
        let mut count = 0usize;
        for y in row.saturating_sub(1)..=(row + 1).min(height.saturating_sub(1)) {
            if is_light_text_pixel(screenshot, x, y) {
                count += 1;
            }
        }
        *bright_slot = count >= 2;
    }

    let first_x = bright.iter().position(|value| *value)?;
    let last_x = bright.iter().rposition(|value| *value)?;
    if last_x <= first_x + 24 {
        return None;
    }

    let mut bright_count = 0u32;
    let mut max_dark_gap = 0u32;
    let mut current_dark_gap = 0u32;
    for is_bright in &bright[first_x..=last_x] {
        if *is_bright {
            bright_count += 1;
            max_dark_gap = max_dark_gap.max(current_dark_gap);
            current_dark_gap = 0;
        } else {
            current_dark_gap += 1;
        }
    }
    max_dark_gap = max_dark_gap.max(current_dark_gap);

    let interior_width = (last_x - first_x + 1) as u32;
    Some(StrikeoutMetrics {
        row: row as u32,
        first_x: first_x as u32,
        last_x: last_x as u32,
        coverage: bright_count as f32 / interior_width as f32,
        max_dark_gap,
    })
}

fn is_light_text_pixel(screenshot: &cranpose::RobotScreenshot, x: usize, y: usize) -> bool {
    let index = (y * screenshot.width as usize + x) * 4;
    let red = screenshot.pixels[index];
    let green = screenshot.pixels[index + 1];
    let blue = screenshot.pixels[index + 2];
    let alpha = screenshot.pixels[index + 3];
    alpha >= 180 && red >= 145 && green >= 145 && blue >= 145
}
