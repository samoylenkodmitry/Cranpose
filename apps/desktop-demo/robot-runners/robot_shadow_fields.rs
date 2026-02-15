//! Robot test validating GraphicsLayer shadow fields produce visible pixel changes.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_shadow_fields --features robot-app`

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, changed_pixel_count, changed_pixel_count_in_region, find_bounds_by_text,
    find_button_in_semantics, find_text_by_prefix_in_semantics, root_bounds,
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
const OUTPUT_DIR: &str = "/tmp/cranpose_robot_shadow_fields";

fn rect_contains(bounds: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= bounds.0 && x < bounds.0 + bounds.2 && y >= bounds.1 && y < bounds.1 + bounds.3
}

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

    let outer_left = (inner_rect.0 - outer_margin).max(0.0).floor() as u32;
    let outer_top = (inner_rect.1 - outer_margin).max(0.0).floor() as u32;
    let outer_right = (inner_rect.0 + inner_rect.2 + outer_margin)
        .min(before.width as f32)
        .ceil() as u32;
    let outer_bottom = (inner_rect.1 + inner_rect.3 + outer_margin)
        .min(before.height as f32)
        .ceil() as u32;

    if outer_right <= outer_left || outer_bottom <= outer_top {
        return 0;
    }

    let width = before.width as usize;
    let mut changed = 0usize;
    for y in outer_top..outer_bottom {
        for x in outer_left..outer_right {
            let fx = x as f32;
            let fy = y as f32;
            if rect_contains(inner_rect, fx, fy) {
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
    let inner = ring_rect;
    let outer = (
        inner.0 - SHADOW_RING_MARGIN,
        inner.1 - SHADOW_RING_MARGIN,
        inner.2 + SHADOW_RING_MARGIN * 2.0,
        inner.3 + SHADOW_RING_MARGIN * 2.0,
    );

    for y in 0..before.height {
        for x in 0..before.width {
            let idx = ((y * before.width + x) * 4) as usize;
            let dr = before.pixels[idx].abs_diff(after.pixels[idx]);
            let dg = before.pixels[idx + 1].abs_diff(after.pixels[idx + 1]);
            let db = before.pixels[idx + 2].abs_diff(after.pixels[idx + 2]);
            let da = before.pixels[idx + 3].abs_diff(after.pixels[idx + 3]);
            let dmax = dr.max(dg).max(db).max(da);
            let in_outer = rect_contains(outer, x as f32, y as f32);
            let in_inner = rect_contains(inner, x as f32, y as f32);
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

fn scroll_down(robot: &cranpose::Robot) {
    cranpose_testing::scroll_down(robot, 620.0, 760.0, 220.0);
}

fn scroll_up(robot: &cranpose::Robot) {
    cranpose_testing::scroll_up(robot, 620.0, 220.0, 760.0);
}

fn y_is_visible(robot: &cranpose::Robot, y: f32) -> bool {
    cranpose_testing::y_is_visible(robot, y)
}

fn scroll_text_into_view(
    robot: &cranpose::Robot,
    text: &str,
    max_attempts: usize,
) -> Option<(f32, f32, f32, f32)> {
    for attempt in 0..max_attempts {
        if let Some(bounds) = find_bounds_by_text(robot, text) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if y_is_visible(robot, center_y) {
                return Some(bounds);
            }
            let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
                return Some(bounds);
            };
            let viewport_mid = root_y + root_h * 0.5;
            if center_y > viewport_mid {
                scroll_down(robot);
            } else {
                scroll_up(robot);
            }
        } else {
            if attempt % 2 == 0 {
                scroll_down(robot);
            } else {
                scroll_up(robot);
            }
        }
    }
    None
}

fn scroll_prefix_into_view(
    robot: &cranpose::Robot,
    prefix: &str,
    max_attempts: usize,
) -> Option<(f32, f32, f32, f32, String)> {
    for attempt in 0..max_attempts {
        if let Some(bounds) = find_text_by_prefix_in_semantics(robot, prefix) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if y_is_visible(robot, center_y) {
                return Some(bounds);
            }
            let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
                return Some(bounds);
            };
            let viewport_mid = root_y + root_h * 0.5;
            if center_y > viewport_mid {
                scroll_down(robot);
            } else {
                scroll_up(robot);
            }
        } else {
            if attempt % 2 == 0 {
                scroll_down(robot);
            } else {
                scroll_up(robot);
            }
        }
    }
    None
}

fn parse_slider_value(text: &str) -> Option<f32> {
    cranpose_testing::parse_slider_value(text)
}

fn set_slider_fraction(robot: &cranpose::Robot, prefix: &str, fraction: f32) -> Option<f32> {
    cranpose_testing::set_slider_fraction(
        robot,
        prefix,
        fraction,
        SHADOW_SLIDER_WIDTH,
        SLIDER_TOUCH_OFFSET_Y,
        cranpose_testing::ScrollConfig {
            center_x: 620.0,
            down_from_y: 760.0,
            down_to_y: 220.0,
            up_from_y: 220.0,
            up_to_y: 760.0,
        },
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

            if scroll_text_into_view(&robot, "GraphicsLayer Fields", 18).is_none() {
                println!("FATAL: could not find 'GraphicsLayer Fields'");
                let _ = robot.exit();
                std::process::exit(1);
            }

            if scroll_text_into_view(&robot, "Shadow Fields", 40).is_none()
                && scroll_prefix_into_view(&robot, "shadow_elevation", 40).is_none()
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

            let shadow_label_bounds = scroll_text_into_view(&robot, "shadow", 14).or_else(|| {
                // Fallback: derive right-preview label area from the stable "none" label.
                scroll_text_into_view(&robot, "none", 10).map(|none| {
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

            let _ = fs::create_dir_all(OUTPUT_DIR);
            let before_path = Path::new(OUTPUT_DIR).join("shadow_before.png");
            let after_path = Path::new(OUTPUT_DIR).join("shadow_after.png");
            let diff_path = Path::new(OUTPUT_DIR).join("shadow_diff.png");
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
