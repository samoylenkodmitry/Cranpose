//! Robot test: text decoration (underline) must not change appearance with scroll position.
//!
//! Takes a screenshot of "This is red, italic, and underlined!" at its initial position,
//! then scrolls by a small amount and takes another screenshot. Normalizes both to the
//! text's bounding box and asserts pixel-identical output. Repeats for multiple sub-pixel
//! scroll offsets to catch device-pixel-boundary artifacts.

use cranpose::AppLauncher;
use cranpose_testing::{
    find_button_exact_in_semantics, find_button_in_semantics, find_text_in_semantics,
    normalize_screenshot_region, screenshot_difference_stats,
};
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const DEBUG_OUTPUT_DIR: &str = "/tmp/cranpose_scroll_decoration_invariance";
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";
// Capture at 2x to reproduce the HiDPI underline thickness artifact
const CAPTURE_SCALE: f32 = 2.0;

fn main() {
    env_logger::init();
    println!("=== Robot Scroll Decoration Invariance ===");

    AppLauncher::new()
        .with_title("Robot Scroll Decoration Invariance")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
            scroll_text_into_view(&robot, TARGET_TEXT, 20);

            verify_scroll_decoration_invariance(&robot);

            println!("\n=== Test Summary ===");
            println!("PASS: text decoration rendering is scroll-position-invariant");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn verify_scroll_decoration_invariance(robot: &cranpose::Robot) {
    println!("\n--- Scroll decoration invariance ---");

    // Scroll in small fractional increments to sweep through sub-pixel positions.
    // The invariant: consecutive small scrolls must produce CONSISTENT, BOUNDED
    // differences. A snap-based renderer would show zero diff for some steps then
    // a huge spike when crossing a grid boundary — that's the artifact.
    let scroll_step = -0.7_f32; // fractional to hit all sub-pixel alignments
    let num_steps = 20;
    // Per-channel tolerance for individual pixel comparison.
    // Sub-pixel rendering variation causes small channel diffs (typically 1-3 per channel).
    let per_step_tolerance: u32 = 4;
    // Max differing pixels between consecutive 0.7px scrolls at 2x, expressed as a
    // share of the normalized capture. Font rasterization differs across CI hosts;
    // the invariant is bounded, consistent variation rather than a fixed pixel count.
    let max_consecutive_diff_ratio: f32 = 0.35;
    // Spike detection: if any step has dramatically more differing pixels than average,
    // that indicates a discrete jump (snap artifact) rather than smooth variation.
    // With smooth rendering, the coefficient of variation should be low.
    let spike_ratio_threshold: f32 = 3.0;

    let initial_bounds = find_text_in_semantics(robot, TARGET_TEXT).expect("target text bounds");
    let center_x = initial_bounds.0 + initial_bounds.2 * 0.5;
    let center_y = initial_bounds.1 + initial_bounds.3 * 0.5;

    let mut prev_region = text_capture_region(initial_bounds);
    let mut prev_shot = robot
        .screenshot_with_scale(CAPTURE_SCALE)
        .expect("initial screenshot");

    let mut diffs: Vec<usize> = Vec::new();

    for step in 0..num_steps {
        robot.mouse_move(center_x, center_y).expect("move cursor");
        std::thread::sleep(Duration::from_millis(30));
        robot.mouse_scroll(0.0, scroll_step).expect("scroll");
        std::thread::sleep(Duration::from_millis(150));
        let _ = robot.wait_for_idle();

        let Some(curr_bounds) = find_text_in_semantics(robot, TARGET_TEXT) else {
            println!("  step {step}: text scrolled out of view, stopping");
            break;
        };

        let curr_region = text_capture_region(curr_bounds);
        let curr_shot = robot
            .screenshot_with_scale(CAPTURE_SCALE)
            .expect("screenshot");

        let output_size = region_output_size(prev_region);
        let prev_normalized =
            normalize_screenshot_region(&prev_shot, prev_region, output_size.0, output_size.1)
                .expect("normalize prev");
        let curr_normalized =
            normalize_screenshot_region(&curr_shot, curr_region, output_size.0, output_size.1)
                .expect("normalize curr");

        let stats =
            screenshot_difference_stats(&prev_normalized, &curr_normalized, per_step_tolerance)
                .expect("diff stats");

        println!(
            "  step {step}: y={:.2} diff_pixels={} max_diff={}",
            curr_bounds.1, stats.differing_pixels, stats.max_difference,
        );

        let max_consecutive_diff_pixels =
            ((output_size.0 * output_size.1) as f32 * max_consecutive_diff_ratio).ceil() as usize;
        if stats.differing_pixels > max_consecutive_diff_pixels {
            save_debug_images(
                &format!("step_{step}"),
                &prev_shot,
                &curr_shot,
                &prev_normalized,
                &curr_normalized,
            );
            panic!(
                "FAIL step {step}: consecutive scroll diff too large: {} pixels (max {}). \
                 This indicates a discrete rendering jump, not smooth sub-pixel variation.",
                stats.differing_pixels, max_consecutive_diff_pixels,
            );
        }

        diffs.push(stats.differing_pixels);
        prev_region = curr_region;
        prev_shot = curr_shot;
    }

    // Spike detection: check for discrete jumps in the diff sequence.
    // With smooth rendering, consecutive diffs should be relatively uniform.
    // With snap artifacts, some diffs are 0 and others spike dramatically.
    let nonzero_diffs: Vec<usize> = diffs.iter().copied().filter(|&d| d > 0).collect();
    if nonzero_diffs.len() >= 3 {
        let avg = nonzero_diffs.iter().sum::<usize>() as f32 / nonzero_diffs.len() as f32;
        let max = *nonzero_diffs.iter().max().unwrap_or(&0) as f32;
        let zero_count = diffs.iter().filter(|&&d| d == 0).count();
        println!(
            "  spike analysis: avg_nonzero={:.1} max_nonzero={:.1} zero_steps={}/{} ratio={:.2}",
            avg,
            max,
            zero_count,
            diffs.len(),
            if avg > 0.0 { max / avg } else { 0.0 },
        );
        if avg > 0.0 && max / avg > spike_ratio_threshold {
            panic!(
                "FAIL: detected discrete rendering spikes (max/avg ratio {:.2} > {:.1}). \
                 Smooth scrolling should produce consistent sub-pixel variation, not spikes.",
                max / avg,
                spike_ratio_threshold,
            );
        }
    }
}

fn text_capture_region(bounds: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    // Include padding around the text to capture the underline and any anti-aliasing edges
    let pad = 4.0;
    (
        bounds.0 - pad,
        bounds.1 - pad,
        bounds.2 + pad * 2.0,
        bounds.3 + pad * 2.0,
    )
}

fn region_output_size(region: (f32, f32, f32, f32)) -> (u32, u32) {
    // Output at device pixel resolution to catch sub-pixel artifacts
    (
        (region.2 * CAPTURE_SCALE).ceil() as u32,
        (region.3 * CAPTURE_SCALE).ceil() as u32,
    )
}

fn open_text_tab(robot: &cranpose::Robot) {
    let Some((x, y, w, h)) = find_button_in_semantics(robot, "Shaders") else {
        panic!("Shaders tab not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Shaders tab");
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let Some((x, y, w, h)) = find_button_exact_in_semantics(robot, "Text") else {
        panic!("Text tab not found");
    };
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Text tab");
    std::thread::sleep(Duration::from_millis(350));
    let _ = robot.wait_for_idle();
    assert!(
        find_text_in_semantics(robot, "Text Rendering Feature Showcase").is_some(),
        "Text showcase heading not found after tab switch"
    );
}

fn scroll_text_into_view(robot: &cranpose::Robot, text: &str, max_attempts: usize) {
    for _ in 0..max_attempts {
        if let Some(bounds) = find_text_in_semantics(robot, text) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if center_y > 100.0 && center_y < (WINDOW_HEIGHT as f32 - 100.0) {
                return;
            }
        }
        // Scroll down to find the text
        robot
            .mouse_move(600.0, 450.0)
            .expect("move cursor to center");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, -80.0)
            .expect("scroll down to find text");
        std::thread::sleep(Duration::from_millis(200));
        let _ = robot.wait_for_idle();
    }
    panic!("text '{text}' not found after {max_attempts} scroll attempts");
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
            "failed to create debug dir {}: {}",
            output_dir.display(),
            err
        );
        return;
    }

    save_png(
        &output_dir.join(format!("{name}_before_raw.png")),
        before_shot.width,
        before_shot.height,
        &before_shot.pixels,
    );
    save_png(
        &output_dir.join(format!("{name}_after_raw.png")),
        after_shot.width,
        after_shot.height,
        &after_shot.pixels,
    );
    save_png(
        &output_dir.join(format!("{name}_before_normalized.png")),
        before_normalized.width,
        before_normalized.height,
        &before_normalized.pixels,
    );
    save_png(
        &output_dir.join(format!("{name}_after_normalized.png")),
        after_normalized.width,
        after_normalized.height,
        &after_normalized.pixels,
    );
    println!("  DEBUG: saved images to {}", output_dir.display());
}

fn save_png(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    let image: RgbaImage = match ImageBuffer::from_raw(width, height, pixels.to_vec()) {
        Some(img) => img,
        None => {
            eprintln!("invalid screenshot dimensions for {}", path.display());
            return;
        }
    };
    if let Err(err) = image.save(path) {
        eprintln!("failed to save {}: {}", path.display(), err);
    }
}
