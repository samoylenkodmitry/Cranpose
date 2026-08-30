mod output_paths;
mod robot_exit;
mod text_showcase_external_helpers;

use std::{path::Path, time::Duration};

use cranpose::AppLauncher;
use cranpose_testing::{crop_screenshot_logical, find_text_in_semantics};
use desktop_app::app;
use image::RgbaImage;
use text_showcase_external_helpers::{
    capture_x11_window_screenshot, find_window_id, open_text_tab, scroll_text_into_view,
};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const WINDOW_TITLE: &str = "Robot Underline Screenshot";
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";
const MIN_UNDERLINE_COVERAGE: f32 = 0.78;
const MAX_UNDERLINE_GAP_PX: u32 = 2;
const MAX_THICKNESS_DELTA_PX: u32 = 1;
const MAX_SPAN_DELTA_PX: u32 = 3;

#[derive(Clone, Copy, Debug)]
struct UnderlineMetrics {
    row: u32,
    first_x: u32,
    last_x: u32,
    coverage: f32,
    max_red_gap: u32,
    thickness: u32,
}

impl UnderlineMetrics {
    fn span_width(self) -> u32 {
        self.last_x.saturating_sub(self.first_x) + 1
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Underline Screenshot ===");
    let output_dir = output_paths::diagnostic_path("cranpose_underline_screenshots");
    println!("Output dir: {}", output_dir.display());

    std::fs::create_dir_all(&output_dir).expect("create output dir");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
            scroll_text_into_view(&robot, TARGET_TEXT, WINDOW_HEIGHT, 20);
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let window_id = find_window_id(WINDOW_TITLE);
            println!("Window ID: {window_id}");

            let mut baseline: Option<UnderlineMetrics> = None;
            for step in 0..10 {
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();

                let bounds = find_text_in_semantics(&robot, TARGET_TEXT)
                    .expect("target text must be visible");
                println!(
                    "  step {step}: text at y={:.2} (x={:.2}, w={:.2}, h={:.2})",
                    bounds.1, bounds.0, bounds.2, bounds.3
                );

                let full_path = output_dir.join(format!("step_{step:02}_full.png"));
                let full = capture_x11_window_screenshot(
                    &window_id,
                    &full_path,
                    WINDOW_WIDTH as f32,
                    WINDOW_HEIGHT as f32,
                );
                let crop = crop_screenshot_logical(
                    &full,
                    bounds.0 - 12.0,
                    bounds.1 - 12.0,
                    bounds.2 + 24.0,
                    bounds.3 + 24.0,
                )
                .unwrap_or_else(|| robot_exit::fail(&robot, "failed to crop underlined text"));
                let crop_path = output_dir.join(format!("step_{step:02}_underline_crop.png"));
                save_robot_screenshot(&crop, &crop_path);

                let metrics = underline_metrics(&crop).unwrap_or_else(|| {
                    robot_exit::fail(
                        &robot,
                        &format!(
                            "red underline not found in crop at step {step}: {}",
                            crop_path.display()
                        ))
                });
                println!(
                    "  underline step={step}: row={} span={} first_x={} last_x={} coverage={:.3} max_red_gap={} thickness={}",
                    metrics.row,
                    metrics.span_width(),
                    metrics.first_x,
                    metrics.last_x,
                    metrics.coverage,
                    metrics.max_red_gap,
                    metrics.thickness
                );
                assert_underline_quality(&robot, step, metrics);
                if let Some(reference) = baseline {
                    assert_underline_stability(&robot, step, reference, metrics);
                } else {
                    baseline = Some(metrics);
                }

                let center_x = bounds.0 + bounds.2 * 0.5;
                let center_y = bounds.1 + bounds.3 * 0.5;
                robot.mouse_move(center_x, center_y).expect("move cursor");
                std::thread::sleep(Duration::from_millis(50));
                robot.mouse_scroll(0.0, -0.7).expect("scroll");
                std::thread::sleep(Duration::from_millis(300));
                let _ = robot.wait_for_idle();
            }

            println!("\n=== Done ===");
            println!("Screenshots saved to {}/", output_dir.display());
            println!("PASS: underline stayed continuous and stable across external X11 scroll captures");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn assert_underline_quality(robot: &cranpose::Robot, step: usize, metrics: UnderlineMetrics) {
    if metrics.coverage < MIN_UNDERLINE_COVERAGE || metrics.max_red_gap > MAX_UNDERLINE_GAP_PX {
        robot_exit::fail(
            robot,
            &format!(
                "underline is dashed or broken at step {step}: coverage={:.3} max_red_gap={} span={}",
                metrics.coverage,
                metrics.max_red_gap,
                metrics.span_width()
            ));
    }
}

fn assert_underline_stability(
    robot: &cranpose::Robot,
    step: usize,
    baseline: UnderlineMetrics,
    current: UnderlineMetrics,
) {
    let thickness_delta = baseline.thickness.abs_diff(current.thickness);
    let span_delta = baseline.span_width().abs_diff(current.span_width());
    if thickness_delta > MAX_THICKNESS_DELTA_PX || span_delta > MAX_SPAN_DELTA_PX {
        robot_exit::fail(
            robot,
            &format!(
                "underline changed under scroll at step {step}: baseline={baseline:?} current={current:?} thickness_delta={thickness_delta} span_delta={span_delta}",
            ));
    }
}

fn underline_metrics(screenshot: &cranpose::RobotScreenshot) -> Option<UnderlineMetrics> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    if width == 0 || height == 0 {
        return None;
    }

    let start_y = (height as f32 * 0.45).floor() as usize;
    let end_y = (height as f32 * 0.95).ceil() as usize;
    (start_y..end_y.min(height))
        .filter_map(|row| underline_row_metrics(screenshot, row))
        .max_by(|a, b| {
            a.coverage
                .total_cmp(&b.coverage)
                .then_with(|| a.span_width().cmp(&b.span_width()))
                .then_with(|| b.max_red_gap.cmp(&a.max_red_gap))
        })
}

fn underline_row_metrics(
    screenshot: &cranpose::RobotScreenshot,
    row: usize,
) -> Option<UnderlineMetrics> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut red = vec![false; width];
    for (x, red_slot) in red.iter_mut().enumerate() {
        let mut count = 0usize;
        for y in row.saturating_sub(1)..=(row + 1).min(height.saturating_sub(1)) {
            if is_red_decoration_pixel(screenshot, x, y) {
                count += 1;
            }
        }
        *red_slot = count >= 1;
    }

    let first_x = red.iter().position(|value| *value)?;
    let last_x = red.iter().rposition(|value| *value)?;
    if last_x <= first_x + 48 {
        return None;
    }

    let mut red_count = 0u32;
    let mut max_red_gap = 0u32;
    let mut current_gap = 0u32;
    for is_red in &red[first_x..=last_x] {
        if *is_red {
            red_count += 1;
            max_red_gap = max_red_gap.max(current_gap);
            current_gap = 0;
        } else {
            current_gap += 1;
        }
    }
    max_red_gap = max_red_gap.max(current_gap);

    let span_width = (last_x - first_x + 1) as u32;
    Some(UnderlineMetrics {
        row: row as u32,
        first_x: first_x as u32,
        last_x: last_x as u32,
        coverage: red_count as f32 / span_width as f32,
        max_red_gap,
        thickness: underline_thickness_at(screenshot, row, first_x, last_x),
    })
}

fn underline_thickness_at(
    screenshot: &cranpose::RobotScreenshot,
    row: usize,
    first_x: usize,
    last_x: usize,
) -> u32 {
    let height = screenshot.height as usize;
    let center_x = (first_x + last_x) / 2;
    let mut thickness = 0u32;
    let start = row.saturating_sub(4);
    let end = (row + 4).min(height.saturating_sub(1));
    for y in start..=end {
        if is_red_decoration_pixel(screenshot, center_x, y)
            || center_x
                .checked_sub(1)
                .is_some_and(|x| is_red_decoration_pixel(screenshot, x, y))
            || is_red_decoration_pixel(screenshot, (center_x + 1).min(last_x), y)
        {
            thickness += 1;
        }
    }
    thickness
}

fn is_red_decoration_pixel(screenshot: &cranpose::RobotScreenshot, x: usize, y: usize) -> bool {
    let index = (y * screenshot.width as usize + x) * 4;
    let red = screenshot.pixels[index] as u16;
    let green = screenshot.pixels[index + 1] as u16;
    let blue = screenshot.pixels[index + 2] as u16;
    let alpha = screenshot.pixels[index + 3];
    alpha >= 160 && red >= 120 && red > green + 40 && red > blue + 40
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
