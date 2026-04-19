//! Robot regression for rigid subtree motion in the real desktop demo.
//!
//! This covers two renderer invariants with measurable app-level checks:
//! - same scroll offset must render identically before and after pointer release
//! - lazy-list item subtree stability under fractional wheel motion

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, find_bounds_by_text, find_button_exact_in_semantics,
    find_button_in_semantics, find_in_semantics, find_text_exact, find_text_in_semantics,
    normalize_screenshot_region, root_bounds, screenshot_difference_stats, scroll_down, scroll_up,
    y_is_visible,
};
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const TEXT_PIXEL_DIFFERENCE_TOLERANCE: u32 = 0;
const LAZY_PIXEL_DIFFERENCE_TOLERANCE: u32 = 16;
const TEXT_DOWNSAMPLE_FACTOR: u32 = 1;
const LAZY_DOWNSAMPLE_FACTOR: u32 = 4;
const TEXT_MAX_DIFFERING_PIXELS: usize = 0;
const TEXT_MAX_PIXEL_DIFFERENCE: u32 = 0;
const LAZY_MAX_DIFFERING_PIXELS: usize = 24;
const LAZY_MAX_PIXEL_DIFFERENCE: u32 = 56;
// Direct text now translates smoothly through fractional scroll positions, so a normalized
// text-only crop can retain a few per-pixel differences even when the row itself moves rigidly.
const LAZY_TEXT_MAX_DIFFERING_PIXELS: usize = 24;
const LAZY_TEXT_MAX_PIXEL_DIFFERENCE: u32 = 72;
const LAZY_SCROLL_DELTA_Y: f32 = -21.5;
const DEBUG_OUTPUT_DIR: &str = "/tmp/cranpose_translation_contract";
const DECORATED_TEXT_PAD_LEFT: f32 = 8.0;
const DECORATED_TEXT_PAD_RIGHT: f32 = 8.0;
const DECORATED_TEXT_TOP_INSET: f32 = 4.0;
const DECORATED_TEXT_BOTTOM_PAD: f32 = 2.0;

fn main() {
    env_logger::init();
    println!("=== Robot Render Translation Contract ===");

    AppLauncher::new()
        .with_title("Robot Render Translation Contract")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            verify_text_drag_release_contract(&robot);
            verify_lazy_list_drag_release_contract(&robot);
            verify_lazy_list_translation_contract(&robot);

            println!("\n=== Test Summary ===");
            println!("✓ rigid subtree motion preserved in desktop demo surfaces");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn verify_text_drag_release_contract(robot: &cranpose::Robot) {
    println!("\n--- Text drag/release contract ---");
    open_text_tab(robot);
    let start_bounds = scroll_text_into_view(robot, "Decorated shadow text", 18)
        .expect("decorated text bounds before drag");
    let drag_center = center(start_bounds);

    robot
        .mouse_move(drag_center.0, drag_center.1)
        .expect("move cursor to text scroller");
    std::thread::sleep(Duration::from_millis(40));
    robot.mouse_down().expect("mouse down");
    std::thread::sleep(Duration::from_millis(40));

    let drag_steps = 6;
    let per_step_y = 7.0;
    for step in 1..=drag_steps {
        robot
            .mouse_move(drag_center.0, drag_center.1 + per_step_y * step as f32)
            .expect("drag move");
        std::thread::sleep(Duration::from_millis(24));
    }
    let drag_end = (
        drag_center.0,
        drag_center.1 + per_step_y * drag_steps as f32,
    );
    std::thread::sleep(Duration::from_millis(70));
    robot
        .mouse_move(drag_end.0, drag_end.1)
        .expect("hold drag to settle velocity");
    std::thread::sleep(Duration::from_millis(70));

    let before_bounds = find_bounds_by_text(robot, "Decorated shadow text")
        .expect("decorated text bounds during drag");
    let before_region = decorated_text_capture_region(before_bounds);
    let before_shot = capture_screenshot(robot).expect("text screenshot during drag");
    log_render_stats(robot, "decorated_drag");

    robot.mouse_up().expect("mouse up");
    std::thread::sleep(Duration::from_millis(80));
    let _ = robot.wait_for_idle();

    let after_bounds =
        find_bounds_by_text(robot, "Decorated shadow text").expect("decorated text bounds after");
    let after_region = decorated_text_capture_region(after_bounds);
    let after_shot = capture_screenshot(robot).expect("text screenshot after release");
    log_render_stats(robot, "decorated_release");

    assert!(
        (before_bounds.0 - after_bounds.0).abs() <= 0.1
            && (before_bounds.1 - after_bounds.1).abs() <= 0.1,
        "drag/release contract requires same logical offset: before={before_bounds:?} after={after_bounds:?}"
    );

    assert_normalized_region_stable(
        "decorated_text_release",
        &before_shot,
        before_region,
        &after_shot,
        after_region,
        RegionStabilityConfig {
            difference_tolerance: TEXT_PIXEL_DIFFERENCE_TOLERANCE,
            max_differing_pixels: TEXT_MAX_DIFFERING_PIXELS,
            max_pixel_difference: TEXT_MAX_PIXEL_DIFFERENCE,
            downsample_factor: Some(TEXT_DOWNSAMPLE_FACTOR),
        },
    );
}

fn log_render_stats(robot: &cranpose::Robot, stage: &str) {
    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            println!(
                "  render_stats stage={} isolated_layer_renders={} offscreen_acquires={} composite_passes={} blur_passes={} text_passes={} shape_passes={} image_passes={}",
                stage,
                stats.isolated_layer_renders,
                stats.offscreen_acquires,
                stats.composite_passes,
                stats.blur_passes,
                stats.text_passes,
                stats.shape_passes,
                stats.image_passes
            );
            for (index, layer) in stats.top_isolated_layers().enumerate() {
                println!(
                    "  isolated_layer stage={} rank={} node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                    stage,
                    index,
                    layer.node_id,
                    layer.logical_rect.x,
                    layer.logical_rect.y,
                    layer.logical_rect.width,
                    layer.logical_rect.height,
                    layer.width,
                    layer.height,
                    layer.reasons.display()
                );
            }
        }
        Ok(None) => println!("  render_stats stage={} unavailable", stage),
        Err(err) => println!("  render_stats stage={} error={}", stage, err),
    }
}

fn verify_lazy_list_translation_contract(robot: &cranpose::Robot) {
    println!("\n--- Lazy list translation contract ---");
    click_tab(robot, "Lazy List");
    wait_for_text(robot, "Lazy List Demo");

    let target_index = 2usize;
    let before_region = lazy_item_region(robot, target_index).expect("lazy item region before");
    let item_label = format!("Item #{target_index}");
    let hello_label = format!("Hello #{target_index}");
    let before_item_bounds = find_bounds_by_text(robot, &item_label).expect("item label before");
    let before_hello_bounds = find_bounds_by_text(robot, &hello_label).expect("hello label before");
    let before_shot = capture_screenshot(robot).expect("lazy screenshot before scroll");

    scroll_at(robot, center(before_region.row_bounds), LAZY_SCROLL_DELTA_Y);

    let after_region = lazy_item_region(robot, target_index).expect("lazy item region after");
    let after_item_bounds = find_bounds_by_text(robot, &item_label).expect("item label after");
    let after_hello_bounds = find_bounds_by_text(robot, &hello_label).expect("hello label after");
    let after_shot = capture_screenshot(robot).expect("lazy screenshot after scroll");
    println!(
        "  lazy_item before_row={:?} after_row={:?}",
        before_region.row_bounds, after_region.row_bounds,
    );

    let delta_y = before_region.row_bounds.1 - after_region.row_bounds.1;
    assert!(
        delta_y.abs() >= 1.0,
        "lazy item did not move enough under wheel scroll: before_y={:.2} after_y={:.2}",
        before_region.row_bounds.1,
        after_region.row_bounds.1
    );

    assert_normalized_region_stable(
        "lazy_item_label",
        &before_shot,
        padded_region(before_item_bounds, 2.0),
        &after_shot,
        padded_region(after_item_bounds, 2.0),
        RegionStabilityConfig {
            difference_tolerance: LAZY_PIXEL_DIFFERENCE_TOLERANCE,
            max_differing_pixels: LAZY_TEXT_MAX_DIFFERING_PIXELS,
            max_pixel_difference: LAZY_TEXT_MAX_PIXEL_DIFFERENCE,
            downsample_factor: Some(LAZY_DOWNSAMPLE_FACTOR),
        },
    );
    assert_normalized_region_stable(
        "lazy_hello_label",
        &before_shot,
        padded_region(before_hello_bounds, 2.0),
        &after_shot,
        padded_region(after_hello_bounds, 2.0),
        RegionStabilityConfig {
            difference_tolerance: LAZY_PIXEL_DIFFERENCE_TOLERANCE,
            max_differing_pixels: LAZY_TEXT_MAX_DIFFERING_PIXELS,
            max_pixel_difference: LAZY_TEXT_MAX_PIXEL_DIFFERENCE,
            downsample_factor: Some(LAZY_DOWNSAMPLE_FACTOR),
        },
    );
}

fn verify_lazy_list_drag_release_contract(robot: &cranpose::Robot) {
    println!("\n--- Lazy list drag/release contract ---");
    click_tab(robot, "Lazy List");
    wait_for_text(robot, "Lazy List Demo");

    let before_row = lazy_item_region(robot, 2).expect("lazy item region before drag");
    let drag_center = center(before_row.row_bounds);
    robot
        .mouse_move(drag_center.0, drag_center.1)
        .expect("move cursor to lazy item");
    std::thread::sleep(Duration::from_millis(40));
    robot.mouse_down().expect("mouse down on lazy list");
    std::thread::sleep(Duration::from_millis(40));

    let drag_steps = 6;
    let per_step_y = 6.5;
    for step in 1..=drag_steps {
        robot
            .mouse_move(drag_center.0, drag_center.1 + per_step_y * step as f32)
            .expect("drag lazy list");
        std::thread::sleep(Duration::from_millis(24));
    }
    let drag_end = (
        drag_center.0,
        drag_center.1 + per_step_y * drag_steps as f32,
    );
    std::thread::sleep(Duration::from_millis(70));
    robot
        .mouse_move(drag_end.0, drag_end.1)
        .expect("hold lazy drag");
    std::thread::sleep(Duration::from_millis(70));

    let during_drag = lazy_item_region(robot, 2).expect("lazy item during drag");
    let before_shot = capture_screenshot(robot).expect("lazy screenshot during drag");

    robot.mouse_up().expect("mouse up on lazy list");
    std::thread::sleep(Duration::from_millis(80));
    let _ = robot.wait_for_idle();

    let after_release = lazy_item_region(robot, 2).expect("lazy item after release");
    let after_shot = capture_screenshot(robot).expect("lazy screenshot after release");

    assert!(
        (during_drag.row_bounds.0 - after_release.row_bounds.0).abs() <= 0.1
            && (during_drag.row_bounds.1 - after_release.row_bounds.1).abs() <= 0.1,
        "lazy drag/release contract requires same logical offset: during_drag={:?} after_release={:?}",
        during_drag.row_bounds,
        after_release.row_bounds
    );

    assert_normalized_region_stable(
        "lazy_item_release",
        &before_shot,
        during_drag.capture_region,
        &after_shot,
        after_release.capture_region,
        RegionStabilityConfig {
            difference_tolerance: LAZY_PIXEL_DIFFERENCE_TOLERANCE,
            max_differing_pixels: LAZY_MAX_DIFFERING_PIXELS,
            max_pixel_difference: LAZY_MAX_PIXEL_DIFFERENCE,
            downsample_factor: Some(LAZY_DOWNSAMPLE_FACTOR),
        },
    );
}

fn click_tab(robot: &cranpose::Robot, label: &str) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, label) else {
        panic!("tab '{label}' not found");
    };
    robot.click(x + w * 0.5, y + h * 0.5).expect("click tab");
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();
}

fn open_text_tab(robot: &cranpose::Robot) {
    click_tab(robot, "Shaders");
    let Some((x, y, w, h)) = find_button_exact_in_semantics(robot, "Text") else {
        panic!("Text tab not found after moving to right-side tabs");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click text tab");
    std::thread::sleep(Duration::from_millis(350));
    let _ = robot.wait_for_idle();
    assert!(
        find_text_in_semantics(robot, "Text Rendering Feature Showcase").is_some(),
        "Text showcase heading not found after tab switch"
    );
}

fn wait_for_text(robot: &cranpose::Robot, text: &str) {
    for _ in 0..30 {
        if find_bounds_by_text(robot, text).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    panic!("text '{text}' did not appear");
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
                scroll_down(robot, 620.0, 760.0, 220.0);
            } else {
                scroll_up(robot, 620.0, 220.0, 760.0);
            }
        } else if attempt % 2 == 0 {
            scroll_down(robot, 620.0, 760.0, 220.0);
        } else {
            scroll_up(robot, 620.0, 220.0, 760.0);
        }
    }
    None
}

fn scroll_at(robot: &cranpose::Robot, center: (f32, f32), delta_y: f32) {
    robot
        .mouse_move(center.0, center.1)
        .expect("move cursor before scroll");
    std::thread::sleep(Duration::from_millis(50));
    robot
        .mouse_scroll(0.0, delta_y)
        .expect("wheel scroll should succeed");
    std::thread::sleep(Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

struct RegionStabilityConfig {
    difference_tolerance: u32,
    max_differing_pixels: usize,
    max_pixel_difference: u32,
    downsample_factor: Option<u32>,
}

fn assert_normalized_region_stable(
    name: &str,
    before_shot: &cranpose::RobotScreenshot,
    before_region: (f32, f32, f32, f32),
    after_shot: &cranpose::RobotScreenshot,
    after_region: (f32, f32, f32, f32),
    config: RegionStabilityConfig,
) {
    let output_size = region_output_size(before_region);
    let before =
        normalize_screenshot_region(before_shot, before_region, output_size.0, output_size.1)
            .expect("normalize before screenshot");
    let after = normalize_screenshot_region(after_shot, after_region, output_size.0, output_size.1)
        .expect("normalize after screenshot");
    let before = config
        .downsample_factor
        .filter(|factor| *factor > 1)
        .map(|factor| box_downsample_screenshot(&before, factor))
        .unwrap_or(before);
    let after = config
        .downsample_factor
        .filter(|factor| *factor > 1)
        .map(|factor| box_downsample_screenshot(&after, factor))
        .unwrap_or(after);
    let stats = screenshot_difference_stats(&before, &after, config.difference_tolerance)
        .expect("normalized screenshots should have matching size");
    println!(
        "  {} normalized diff: differing_pixels={} max_diff={}",
        name, stats.differing_pixels, stats.max_difference
    );

    if stats.differing_pixels > config.max_differing_pixels
        || stats.max_difference > config.max_pixel_difference
    {
        save_debug_images(name, before_shot, after_shot, &before, &after);
        let diff = stats
            .first_difference
            .as_ref()
            .expect("failing normalized diff should report first difference");
        panic!(
            "{} drifted under rigid translation: differing_pixels={} max_diff={} first_diff=({}, {}) before={:?} after={:?}",
            name,
            stats.differing_pixels,
            stats.max_difference,
            diff.x,
            diff.y,
            diff.before,
            diff.after
        );
    }
}

fn save_debug_images(
    name: &str,
    before_shot: &cranpose::RobotScreenshot,
    after_shot: &cranpose::RobotScreenshot,
    before_normalized: &cranpose::RobotScreenshot,
    after_normalized: &cranpose::RobotScreenshot,
) {
    let output_dir = PathBuf::from(DEBUG_OUTPUT_DIR);
    if let Err(err) = std::fs::create_dir_all(&output_dir) {
        eprintln!(
            "failed to create translation-contract debug output dir {}: {}",
            output_dir.display(),
            err
        );
        return;
    }

    let raw_before = output_dir.join(format!("{name}_before_raw.png"));
    let raw_after = output_dir.join(format!("{name}_after_raw.png"));
    let normalized_before = output_dir.join(format!("{name}_before_normalized.png"));
    let normalized_after = output_dir.join(format!("{name}_after_normalized.png"));

    if let Err(err) = save_png(
        &raw_before,
        before_shot.width,
        before_shot.height,
        &before_shot.pixels,
    ) {
        eprintln!("failed to save {}: {}", raw_before.display(), err);
    }
    if let Err(err) = save_png(
        &raw_after,
        after_shot.width,
        after_shot.height,
        &after_shot.pixels,
    ) {
        eprintln!("failed to save {}: {}", raw_after.display(), err);
    }
    if let Err(err) = save_png(
        &normalized_before,
        before_normalized.width,
        before_normalized.height,
        &before_normalized.pixels,
    ) {
        eprintln!("failed to save {}: {}", normalized_before.display(), err);
    }
    if let Err(err) = save_png(
        &normalized_after,
        after_normalized.width,
        after_normalized.height,
        &after_normalized.pixels,
    ) {
        eprintln!("failed to save {}: {}", normalized_after.display(), err);
    }

    println!(
        "TRANSLATION_CONTRACT_DEBUG name={} dir={}",
        name,
        output_dir.display()
    );
}

fn save_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> Result<(), String> {
    let image: RgbaImage = ImageBuffer::from_raw(width, height, pixels.to_vec())
        .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    image
        .save(path)
        .map_err(|err| format!("failed to save {}: {}", path.display(), err))
}

fn decorated_text_capture_region(bounds: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    (
        bounds.0 - DECORATED_TEXT_PAD_LEFT,
        bounds.1 + DECORATED_TEXT_TOP_INSET,
        bounds.2 + DECORATED_TEXT_PAD_LEFT + DECORATED_TEXT_PAD_RIGHT,
        (bounds.3 - DECORATED_TEXT_TOP_INSET + DECORATED_TEXT_BOTTOM_PAD).max(1.0),
    )
}

fn box_downsample_screenshot(
    screenshot: &cranpose::RobotScreenshot,
    factor: u32,
) -> cranpose::RobotScreenshot {
    let factor = factor.max(1);
    let out_width = (screenshot.width / factor).max(1);
    let out_height = (screenshot.height / factor).max(1);
    let mut pixels = vec![0u8; (out_width * out_height * 4) as usize];

    for out_y in 0..out_height {
        for out_x in 0..out_width {
            let mut accum = [0u32; 4];
            let mut samples = 0u32;
            let start_x = out_x * factor;
            let start_y = out_y * factor;
            let end_x = (start_x + factor).min(screenshot.width);
            let end_y = (start_y + factor).min(screenshot.height);

            for y in start_y..end_y {
                for x in start_x..end_x {
                    let index = ((y * screenshot.width + x) * 4) as usize;
                    accum[0] += screenshot.pixels[index] as u32;
                    accum[1] += screenshot.pixels[index + 1] as u32;
                    accum[2] += screenshot.pixels[index + 2] as u32;
                    accum[3] += screenshot.pixels[index + 3] as u32;
                    samples += 1;
                }
            }

            let dst = ((out_y * out_width + out_x) * 4) as usize;
            pixels[dst] = (accum[0] / samples.max(1)) as u8;
            pixels[dst + 1] = (accum[1] / samples.max(1)) as u8;
            pixels[dst + 2] = (accum[2] / samples.max(1)) as u8;
            pixels[dst + 3] = (accum[3] / samples.max(1)) as u8;
        }
    }

    cranpose::RobotScreenshot {
        width: out_width,
        height: out_height,
        logical_width: screenshot.logical_width,
        logical_height: screenshot.logical_height,
        pixels,
    }
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn region_output_size(region: (f32, f32, f32, f32)) -> (u32, u32) {
    (
        region.2.max(1.0).round() as u32,
        region.3.max(1.0).round() as u32,
    )
}

fn padded_region(bounds: (f32, f32, f32, f32), pad: f32) -> (f32, f32, f32, f32) {
    (
        bounds.0 - pad,
        bounds.1 - pad,
        bounds.2 + pad * 2.0,
        bounds.3 + pad * 2.0,
    )
}

#[derive(Clone, Copy, Debug)]
struct LazyItemRegion {
    row_bounds: (f32, f32, f32, f32),
    capture_region: (f32, f32, f32, f32),
}

fn lazy_item_region(robot: &cranpose::Robot, index: usize) -> Option<LazyItemRegion> {
    let row_label = format!("ItemRow #{index}");
    let item_label = format!("Item #{index}");
    let hello_label = format!("Hello #{index}");
    let row_bounds = find_in_semantics(robot, |elem| find_text_exact(elem, &row_label))?;
    let item_bounds = find_bounds_by_text(robot, &item_label)?;
    let hello_bounds = find_bounds_by_text(robot, &hello_label)?;
    let left = item_bounds.0.min(hello_bounds.0) - 2.0;
    let top = item_bounds.1 - 2.0;
    let right = (item_bounds.0 + item_bounds.2).max(hello_bounds.0 + hello_bounds.2) + 2.0;
    let bottom = hello_bounds.1 + hello_bounds.3 + 2.0;

    Some(LazyItemRegion {
        row_bounds,
        capture_region: (left, top, right - left, bottom - top),
    })
}
