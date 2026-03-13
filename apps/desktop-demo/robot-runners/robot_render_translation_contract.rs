//! Robot regression for rigid subtree motion in the real desktop demo.
//!
//! This covers two `render_arch.md` requirements with one measurable app-level check:
//! - translated text plus shadow plus decoration under scroll
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
const TEXT_PIXEL_DIFFERENCE_TOLERANCE: u32 = 24;
const LAZY_PIXEL_DIFFERENCE_TOLERANCE: u32 = 64;
const TEXT_MAX_DIFFERING_PIXELS: usize = 240;
const TEXT_MAX_PIXEL_DIFFERENCE: u32 = 64;
// Direct scrolling content can still accumulate bounded edge drift because it
// is rasterized in root space at a different fractional phase after scroll.
// The robot contract should reject visible subtree distortion, not demand
// strict pixel identity from the direct path.
const LAZY_MAX_DIFFERING_PIXELS: usize = 96;
const LAZY_MAX_PIXEL_DIFFERENCE: u32 = 160;
const TEXT_SCROLL_DELTA_Y: f32 = -18.5;
const LAZY_SCROLL_DELTA_Y: f32 = -21.5;
const DEBUG_OUTPUT_DIR: &str = "/tmp/cranpose_translation_contract";

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

            verify_text_translation_contract(&robot);
            verify_lazy_list_translation_contract(&robot);

            println!("\n=== Test Summary ===");
            println!("✓ rigid subtree motion preserved in desktop demo surfaces");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn verify_text_translation_contract(robot: &cranpose::Robot) {
    println!("\n--- Text translation contract ---");
    open_text_tab(robot);
    let before_bounds =
        scroll_text_into_view(robot, "Decorated shadow text", 18).expect("decorated text bounds");
    let before_region = pad_bounds(before_bounds, 20.0, 18.0);
    let before_shot = capture_screenshot(robot).expect("text screenshot before scroll");

    scroll_at(robot, center(before_bounds), TEXT_SCROLL_DELTA_Y);

    let after_bounds =
        find_bounds_by_text(robot, "Decorated shadow text").expect("decorated text bounds after");
    let after_region = pad_bounds(after_bounds, 20.0, 18.0);
    let after_shot = capture_screenshot(robot).expect("text screenshot after scroll");

    let delta_y = before_bounds.1 - after_bounds.1;
    assert!(
        delta_y.abs() >= 1.0,
        "text sample did not move enough under scroll: before_y={:.2} after_y={:.2}",
        before_bounds.1,
        after_bounds.1
    );

    assert_normalized_region_stable(
        "decorated_text",
        &before_shot,
        before_region,
        &after_shot,
        after_region,
        TEXT_PIXEL_DIFFERENCE_TOLERANCE,
        TEXT_MAX_DIFFERING_PIXELS,
        TEXT_MAX_PIXEL_DIFFERENCE,
    );
}

fn verify_lazy_list_translation_contract(robot: &cranpose::Robot) {
    println!("\n--- Lazy list translation contract ---");
    click_tab(robot, "Lazy List");
    wait_for_text(robot, "Lazy List Demo");

    let target_index = 2usize;
    let before_region = lazy_item_region(robot, target_index).expect("lazy item region before");
    let before_shot = capture_screenshot(robot).expect("lazy screenshot before scroll");

    scroll_at(robot, center(before_region.row_bounds), LAZY_SCROLL_DELTA_Y);

    let after_region = lazy_item_region(robot, target_index).expect("lazy item region after");
    let after_shot = capture_screenshot(robot).expect("lazy screenshot after scroll");

    let delta_y = before_region.row_bounds.1 - after_region.row_bounds.1;
    assert!(
        delta_y.abs() >= 1.0,
        "lazy item did not move enough under wheel scroll: before_y={:.2} after_y={:.2}",
        before_region.row_bounds.1,
        after_region.row_bounds.1
    );

    assert_normalized_region_stable(
        "lazy_item",
        &before_shot,
        before_region.capture_region,
        &after_shot,
        after_region.capture_region,
        LAZY_PIXEL_DIFFERENCE_TOLERANCE,
        LAZY_MAX_DIFFERING_PIXELS,
        LAZY_MAX_PIXEL_DIFFERENCE,
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

fn assert_normalized_region_stable(
    name: &str,
    before_shot: &cranpose::RobotScreenshot,
    before_region: (f32, f32, f32, f32),
    after_shot: &cranpose::RobotScreenshot,
    after_region: (f32, f32, f32, f32),
    difference_tolerance: u32,
    max_differing_pixels: usize,
    max_pixel_difference: u32,
) {
    let output_size = region_output_size(before_region);
    let before =
        normalize_screenshot_region(before_shot, before_region, output_size.0, output_size.1)
            .expect("normalize before screenshot");
    let after = normalize_screenshot_region(after_shot, after_region, output_size.0, output_size.1)
        .expect("normalize after screenshot");
    let stats = screenshot_difference_stats(&before, &after, difference_tolerance)
        .expect("normalized screenshots should have matching size");
    println!(
        "  {} normalized diff: differing_pixels={} max_diff={}",
        name, stats.differing_pixels, stats.max_difference
    );

    if stats.differing_pixels > max_differing_pixels || stats.max_difference > max_pixel_difference
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

fn pad_bounds(bounds: (f32, f32, f32, f32), pad_x: f32, pad_y: f32) -> (f32, f32, f32, f32) {
    (
        bounds.0 - pad_x,
        bounds.1 - pad_y,
        bounds.2 + pad_x * 2.0,
        bounds.3 + pad_y * 2.0,
    )
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

#[derive(Clone, Copy, Debug)]
struct LazyItemRegion {
    row_bounds: (f32, f32, f32, f32),
    capture_region: (f32, f32, f32, f32),
}

fn lazy_item_region(robot: &cranpose::Robot, index: usize) -> Option<LazyItemRegion> {
    let row_label = format!("ItemRow #{index}");
    let hello_label = format!("Hello #{index}");
    let row_bounds = find_in_semantics(robot, |elem| find_text_exact(elem, &row_label))?;
    let hello_bounds = find_bounds_by_text(robot, &hello_label)?;
    let left = row_bounds.0 - 16.0;
    let top = row_bounds.1 - 12.0;
    let right = (row_bounds.0 + 240.0).max(hello_bounds.0 + hello_bounds.2 + 16.0);
    let bottom = hello_bounds.1 + hello_bounds.3 + 12.0;

    Some(LazyItemRegion {
        row_bounds,
        capture_region: (left, top, right - left, bottom - top),
    })
}
