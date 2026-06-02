//! Robot test validating GraphicsLayer shadow fields produce visible pixel changes.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_shadow_fields --features robot-app`

mod output_paths;

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, changed_pixel_count, changed_pixel_count_in_region,
    find_button_in_semantics, logical_region_to_pixel_bounds, scroll_prefix_into_view,
    scroll_text_into_view, ScrollConfig,
};
use desktop_app::app;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::fs;
use std::path::Path;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 900;
const CHANNEL_THRESHOLD: u8 = 10;
const SHADOW_SLIDER_WIDTH: f32 = 196.0;
const SLIDER_TOUCH_OFFSET_Y: f32 = 9.0;
const SHADOW_RECT_WIDTH: f32 = 68.0;
const SHADOW_RECT_HEIGHT: f32 = 56.0;
const SHADOW_LABEL_TO_RECT_X: f32 = 16.0;
const SHADOW_LABEL_TO_RECT_Y: f32 = 20.0;
const NONE_LABEL_TO_SHADOW_LABEL_X: f32 = 86.0;
const SHADOW_RING_MARGIN: f32 = 40.0;
const SHADOW_RING_MIN_PIXELS: usize = 180;

fn changed_pixel_count_in_ring(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    inner_rect: (f32, f32, f32, f32),
    outer_margin: f32,
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    let Some((inner_left, inner_top, inner_right, inner_bottom)) =
        logical_region_to_pixel_bounds(before, inner_rect)
    else {
        return 0;
    };
    let Some((outer_left, outer_top, outer_right, outer_bottom)) = logical_region_to_pixel_bounds(
        before,
        (
            inner_rect.0 - outer_margin,
            inner_rect.1 - outer_margin,
            inner_rect.2 + outer_margin * 2.0,
            inner_rect.3 + outer_margin * 2.0,
        ),
    ) else {
        return 0;
    };

    let width = before.width as usize;
    let mut changed = 0usize;
    for y in outer_top..outer_bottom {
        for x in outer_left..outer_right {
            if x >= inner_left && x < inner_right && y >= inner_top && y < inner_bottom {
                continue;
            }
            let idx = ((y as usize) * width + x as usize) * 4;
            if before.pixels[idx].abs_diff(after.pixels[idx]) > channel_threshold
                || before.pixels[idx + 1].abs_diff(after.pixels[idx + 1]) > channel_threshold
                || before.pixels[idx + 2].abs_diff(after.pixels[idx + 2]) > channel_threshold
                || before.pixels[idx + 3].abs_diff(after.pixels[idx + 3]) > channel_threshold
            {
                changed += 1;
            }
        }
    }

    changed
}

fn save_png(path: &Path, screenshot: &cranpose::RobotScreenshot) -> Result<(), String> {
    let img: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    img.save(path)
        .map_err(|e| format!("failed to save {}: {}", path.display(), e))
}

fn build_diff_image(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    ring_rect: (f32, f32, f32, f32),
) -> Option<RgbaImage> {
    if before.width != after.width || before.height != after.height {
        return None;
    }
    let mut out: RgbaImage = ImageBuffer::new(before.width, before.height);
    let outer = (
        ring_rect.0 - SHADOW_RING_MARGIN,
        ring_rect.1 - SHADOW_RING_MARGIN,
        ring_rect.2 + SHADOW_RING_MARGIN * 2.0,
        ring_rect.3 + SHADOW_RING_MARGIN * 2.0,
    );
    let inner_bounds = logical_region_to_pixel_bounds(before, ring_rect)?;
    let outer_bounds = logical_region_to_pixel_bounds(before, outer)?;

    for y in 0..before.height {
        for x in 0..before.width {
            let idx = ((y * before.width + x) * 4) as usize;
            let dr = before.pixels[idx].abs_diff(after.pixels[idx]);
            let dg = before.pixels[idx + 1].abs_diff(after.pixels[idx + 1]);
            let db = before.pixels[idx + 2].abs_diff(after.pixels[idx + 2]);
            let da = before.pixels[idx + 3].abs_diff(after.pixels[idx + 3]);
            let dmax = dr.max(dg).max(db).max(da);
            let in_outer = x >= outer_bounds.0
                && x < outer_bounds.2
                && y >= outer_bounds.1
                && y < outer_bounds.3;
            let in_inner = x >= inner_bounds.0
                && x < inner_bounds.2
                && y >= inner_bounds.1
                && y < inner_bounds.3;
            let px = if in_outer && !in_inner {
                if dmax > CHANNEL_THRESHOLD {
                    let intensity = dmax.max(80);
                    Rgba([255, intensity, 0, 255])
                } else {
                    Rgba([20, 20, 20, 255])
                }
            } else {
                // Dim grayscale context for easier visual inspection.
                let g = ((before.pixels[idx] as u16
                    + before.pixels[idx + 1] as u16
                    + before.pixels[idx + 2] as u16)
                    / 3) as u8;
                Rgba([g / 3, g / 3, g / 3, 255])
            };
            out.put_pixel(x, y, px);
        }
    }

    Some(out)
}

fn shadow_rect_from_label(shadow_label_bounds: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (
        shadow_label_bounds.0 - SHADOW_LABEL_TO_RECT_X,
        shadow_label_bounds.1 - SHADOW_LABEL_TO_RECT_Y,
        SHADOW_RECT_WIDTH,
        SHADOW_RECT_HEIGHT,
    )
}

fn shadow_scroll_config() -> ScrollConfig {
    ScrollConfig {
        center_x: 620.0,
        down_from_y: 760.0,
        down_to_y: 220.0,
        up_from_y: 220.0,
        up_to_y: 760.0,
    }
}

fn set_slider_fraction(robot: &cranpose::Robot, prefix: &str, fraction: f32) -> Option<f32> {
    cranpose_testing::set_slider_fraction(
        robot,
        prefix,
        fraction,
        SHADOW_SLIDER_WIDTH,
        SLIDER_TOUCH_OFFSET_Y,
        shadow_scroll_config(),
    )
}

fn set_shadow_slider_fraction(robot: &cranpose::Robot, fraction: f32) -> Option<f32> {
    set_slider_fraction(robot, "shadow_elevation:", fraction)
        .or_else(|| set_slider_fraction(robot, "shadow_elevation", fraction))
}

fn shadow_preview_region(shadow_label_bounds: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = shadow_label_bounds;
    // Keep region around the right preview rectangle only.
    ((x - 36.0).max(0.0), (y - 34.0).max(0.0), w + 70.0, h + 68.0)
}

fn main() {
    env_logger::init();
    println!("=== Robot Shadow Fields Visual Test ===");

    AppLauncher::new()
        .with_title("Robot Shadow Fields Visual Test")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();

            let Some((tab_x, tab_y, tab_w, tab_h)) = find_button_in_semantics(&robot, "Shaders")
            else {
                println!("FATAL: could not find 'Shaders' tab");
                let _ = robot.exit();
                std::process::exit(1);
            };
            let _ = robot.click(tab_x + tab_w * 0.5, tab_y + tab_h * 0.5);
            std::thread::sleep(Duration::from_millis(320));
            let _ = robot.wait_for_idle();

            if scroll_text_into_view(&robot, "GraphicsLayer Fields", 18, shadow_scroll_config())
                .is_none()
            {
                println!("FATAL: could not find 'GraphicsLayer Fields'");
                let _ = robot.exit();
                std::process::exit(1);
            }

            if scroll_text_into_view(&robot, "Shadow Fields", 40, shadow_scroll_config()).is_none()
                && scroll_prefix_into_view(&robot, "shadow_elevation", 40, shadow_scroll_config())
                    .is_none()
            {
                println!("FATAL: could not find 'Shadow Fields'");
                let _ = robot.exit();
                std::process::exit(1);
            }

            let shadow_off = set_shadow_slider_fraction(&robot, 0.0);
            println!("shadow_elevation after off scrub: {:?}", shadow_off);

            // Max out color alpha so shadow visibility depends mostly on elevation.
            let ambient_alpha = set_slider_fraction(&robot, "ambient_alpha", 1.0);
            let spot_alpha = set_slider_fraction(&robot, "spot_alpha", 1.0);
            println!(
                "ambient_alpha={:?} spot_alpha={:?}",
                ambient_alpha, spot_alpha
            );

            let shadow_label_bounds =
                scroll_text_into_view(&robot, "shadow", 14, shadow_scroll_config()).or_else(|| {
                    // Fallback: derive right-preview label area from the stable "none" label.
                    scroll_text_into_view(&robot, "none", 10, shadow_scroll_config()).map(|none| {
                        (
                            none.0 + NONE_LABEL_TO_SHADOW_LABEL_X,
                            none.1,
                            none.2,
                            none.3,
                        )
                    })
                });
            let Some(shadow_label_bounds) = shadow_label_bounds else {
                println!("FATAL: could not find 'shadow' label in preview");
                let _ = robot.exit();
                std::process::exit(1);
            };
            let shadow_rect = shadow_rect_from_label(shadow_label_bounds);
            let region = shadow_preview_region(shadow_label_bounds);

            // Baseline noise in the same region.
            let Some(base_a) = capture_screenshot(&robot) else {
                println!("FATAL: failed to capture baseline screenshot A");
                let _ = robot.exit();
                std::process::exit(1);
            };
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();
            let Some(base_b) = capture_screenshot(&robot) else {
                println!("FATAL: failed to capture baseline screenshot B");
                let _ = robot.exit();
                std::process::exit(1);
            };
            let baseline_noise =
                changed_pixel_count_in_region(&base_a, &base_b, region, CHANNEL_THRESHOLD);
            let baseline_ring_noise = changed_pixel_count_in_ring(
                &base_a,
                &base_b,
                shadow_rect,
                SHADOW_RING_MARGIN,
                CHANNEL_THRESHOLD,
            );

            let shadow_on = set_shadow_slider_fraction(&robot, 1.0);
            println!("shadow_elevation after on scrub: {:?}", shadow_on);

            // Move pointer away from controls before capture to reduce hover artifacts.
            let _ = robot.click(24.0, 24.0);
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.wait_for_idle();

            let Some(after_on) = capture_screenshot(&robot) else {
                println!("FATAL: failed to capture shadow-on screenshot");
                let _ = robot.exit();
                std::process::exit(1);
            };

            let full_changed = changed_pixel_count(&base_b, &after_on, CHANNEL_THRESHOLD);
            let raw_changed =
                changed_pixel_count_in_region(&base_b, &after_on, region, CHANNEL_THRESHOLD);
            let net_changed = raw_changed.saturating_sub(baseline_noise);
            let raw_ring_changed = changed_pixel_count_in_ring(
                &base_b,
                &after_on,
                shadow_rect,
                SHADOW_RING_MARGIN,
                CHANNEL_THRESHOLD,
            );
            let net_ring_changed = raw_ring_changed.saturating_sub(baseline_ring_noise);

            let output_dir = output_paths::diagnostic_path("cranpose_robot_shadow_fields");
            let _ = fs::create_dir_all(&output_dir);
            let before_path = output_dir.join("shadow_before.png");
            let after_path = output_dir.join("shadow_after.png");
            let diff_path = output_dir.join("shadow_diff.png");
            if let Err(err) = save_png(&before_path, &base_b) {
                println!("WARN: {}", err);
            }
            if let Err(err) = save_png(&after_path, &after_on) {
                println!("WARN: {}", err);
            }
            if let Some(diff_img) = build_diff_image(&base_b, &after_on, shadow_rect) {
                if let Err(err) = diff_img.save(&diff_path) {
                    println!("WARN: failed to save {}: {}", diff_path.display(), err);
                }
            }

            println!(
                "shadow pixel diff: full={} region(raw/baseline/net)={}/{}/{} ring(raw/baseline/net)={}/{}/{}",
                full_changed,
                raw_changed,
                baseline_noise,
                net_changed,
                raw_ring_changed,
                baseline_ring_noise,
                net_ring_changed,
            );
            println!(
                "shadow rect=({:.1},{:.1},{:.1},{:.1}) pngs: {} {} {}",
                shadow_rect.0,
                shadow_rect.1,
                shadow_rect.2,
                shadow_rect.3,
                before_path.display(),
                after_path.display(),
                diff_path.display(),
            );

            if net_ring_changed < SHADOW_RING_MIN_PIXELS {
                println!(
                    "FATAL: shadow field slider did not produce visible shadow pixels in ring (net_ring_changed={})",
                    net_ring_changed
                );
                let _ = robot.exit();
                std::process::exit(1);
            }

            println!("PASS: Shadow Fields demo shows visible pixel changes from shadow controls");
            let _ = robot.exit();
        })
        .run(app::combined_app);
}
