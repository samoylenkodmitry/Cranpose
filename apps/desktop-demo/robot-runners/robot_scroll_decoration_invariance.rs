mod output_paths;

use std::{path::Path, time::Duration};

use cranpose::{AppLauncher, RobotScreenshot, SemanticElement};
use cranpose_testing::{
    find_button_exact_in_semantics, find_button_in_semantics, find_text_in_semantics,
    normalize_screenshot_region, screenshot_difference_stats,
};
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";
const CAPTURE_SCALE: f32 = 2.0;
const UNDERLINE_TRACK_STEPS: usize = 12;
const UNDERLINE_TRACK_SCROLL_DELTA_Y: f32 = -0.7;
const UNDERLINE_LOCAL_Y_SPREAD: f32 = 0.5;
const UNDERLINE_LOCAL_Y_TREND: f32 = 0.25;
const MIN_NORMALIZED_TEXT_INK_PIXELS: usize = 500;

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

            verify_underline_row_tracks_fractional_scroll(&robot);
            verify_scroll_decoration_invariance(&robot);

            println!("\n=== Test Summary ===");
            println!("PASS: text decoration rendering is scroll-position-invariant");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

#[derive(Clone, Copy, Debug)]
struct UnderlineRowMetrics {
    center_y: f32,
    red_pixels: usize,
    max_run: usize,
}

fn verify_underline_row_tracks_fractional_scroll(robot: &cranpose::Robot) {
    println!("\n--- Underline row tracks fractional scroll ---");

    let mut samples = Vec::with_capacity(UNDERLINE_TRACK_STEPS + 1);
    for step in 0..=UNDERLINE_TRACK_STEPS {
        let bounds = find_text_in_semantics(robot, TARGET_TEXT).expect("target text bounds");
        let shot = robot
            .screenshot_with_scale(CAPTURE_SCALE)
            .expect("underline tracking screenshot");
        let metrics = find_presented_underline_row(&shot, bounds).expect("presented underline row");
        let scale_y = shot.height as f32 / shot.logical_height.max(1.0);
        let local_y = metrics.center_y / scale_y - bounds.1;
        println!(
            "  step {step}: semantic_y={:.2} underline_local_y={local_y:.3} red_pixels={} max_run={}",
            bounds.1, metrics.red_pixels, metrics.max_run
        );
        samples.push(local_y);

        if step < UNDERLINE_TRACK_STEPS {
            let center_x = bounds.0 + bounds.2 * 0.5;
            let center_y = bounds.1 + bounds.3 * 0.5;
            robot.mouse_move(center_x, center_y).expect("move cursor");
            std::thread::sleep(Duration::from_millis(30));
            robot
                .mouse_scroll_and_wait_for_frame(0.0, UNDERLINE_TRACK_SCROLL_DELTA_Y)
                .expect("fractional underline tracking scroll");
            std::thread::sleep(Duration::from_millis(150));
            let _ = robot.wait_for_idle();
        }
    }

    let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
    let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let spread = max - min;
    assert!(
        spread <= UNDERLINE_LOCAL_Y_SPREAD,
        "underlined span decoration row swings wider than one snap step: min={min:.3} max={max:.3} spread={spread:.3} samples={samples:?}"
    );

    let half = samples.len() / 2;
    let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len() as f32;
    let first_half = mean(&samples[..half]);
    let second_half = mean(&samples[half..]);
    let trend = (second_half - first_half).abs();
    assert!(
        trend <= UNDERLINE_LOCAL_Y_TREND,
        "underlined span decoration row is walking away from its text: first_half={first_half:.3} second_half={second_half:.3} trend={trend:.3} samples={samples:?}"
    );
}

fn find_presented_underline_row(
    screenshot: &cranpose::RobotScreenshot,
    bounds: (f32, f32, f32, f32),
) -> Option<UnderlineRowMetrics> {
    let scale_x = screenshot.width as f32 / screenshot.logical_width.max(1.0);
    let scale_y = screenshot.height as f32 / screenshot.logical_height.max(1.0);
    let left = ((bounds.0 - 4.0) * scale_x).floor().max(0.0) as usize;
    let right = ((bounds.0 + bounds.2 + 4.0) * scale_x)
        .ceil()
        .min(screenshot.width as f32) as usize;
    let top = ((bounds.1 + bounds.3 * 0.45) * scale_y).floor().max(0.0) as usize;
    let bottom = ((bounds.1 + bounds.3 + 6.0) * scale_y)
        .ceil()
        .min(screenshot.height as f32) as usize;
    if right <= left || bottom <= top {
        return None;
    }

    let mut rows = Vec::new();
    for y in top..bottom {
        let mut red_pixels = 0usize;
        let mut run = 0usize;
        let mut max_run = 0usize;
        for x in left..right {
            if is_red_decoration_pixel(screenshot, x, y) {
                red_pixels += 1;
                run += 1;
                max_run = max_run.max(run);
            } else {
                run = 0;
            }
        }
        if max_run >= 16 && red_pixels >= 24 {
            rows.push((y, red_pixels, max_run));
        }
    }

    let best_run = rows.iter().map(|(_, _, max_run)| *max_run).max()?;
    let min_selected_run = (best_run * 3 / 4).max(16);
    let mut weighted_y = 0.0f32;
    let mut total_weight = 0usize;
    let mut selected_pixels = 0usize;
    for (y, red_pixels, max_run) in rows {
        if max_run < min_selected_run {
            continue;
        }
        weighted_y += y as f32 * red_pixels as f32;
        total_weight += red_pixels;
        selected_pixels += red_pixels;
    }

    (total_weight > 0).then_some(UnderlineRowMetrics {
        center_y: weighted_y / total_weight as f32,
        red_pixels: selected_pixels,
        max_run: best_run,
    })
}

fn is_red_decoration_pixel(screenshot: &cranpose::RobotScreenshot, x: usize, y: usize) -> bool {
    let index = (y * screenshot.width as usize + x) * 4;
    let red = screenshot.pixels[index] as u16;
    let green = screenshot.pixels[index + 1] as u16;
    let blue = screenshot.pixels[index + 2] as u16;
    red >= 120 && red > green + 25 && red > blue + 25
}

fn verify_scroll_decoration_invariance(robot: &cranpose::Robot) {
    println!("\n--- Scroll decoration invariance ---");

    let scroll_step = -0.7_f32;
    let num_steps = 20;
    let per_step_tolerance: u32 = 4;
    let max_consecutive_diff_ratio: f32 = 0.35;
    let spike_ratio_threshold: f32 = 3.0;

    let initial_capture = capture_visible_text_region(robot).expect("initial visible text region");
    let center_x = initial_capture.bounds.0 + initial_capture.bounds.2 * 0.5;
    let center_y = initial_capture.bounds.1 + initial_capture.bounds.3 * 0.5;

    let mut prev_region = initial_capture.region;
    let mut prev_shot = initial_capture.screenshot;

    let mut diffs: Vec<usize> = Vec::new();

    for step in 0..num_steps {
        robot.mouse_move(center_x, center_y).expect("move cursor");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll_and_wait_for_frame(0.0, scroll_step)
            .expect("scroll");
        std::thread::sleep(Duration::from_millis(150));
        let _ = robot.wait_for_idle();

        let Some(curr_capture) = capture_visible_text_region(robot) else {
            println!("  step {step}: text scrolled out of view, stopping");
            break;
        };

        let curr_region = curr_capture.region;
        let curr_shot = curr_capture.screenshot;

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
            curr_capture.bounds.1, stats.differing_pixels, stats.max_difference,
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

struct TextRegionCapture {
    bounds: (f32, f32, f32, f32),
    region: (f32, f32, f32, f32),
    screenshot: RobotScreenshot,
}

fn capture_visible_text_region(robot: &cranpose::Robot) -> Option<TextRegionCapture> {
    let mut best_ink = 0usize;
    let mut best_bounds = None;
    let mut best_candidate_count = 0usize;
    let mut first_candidate_bounds = None;
    let mut skipped_out_of_view = 0usize;
    let mut first_screenshot = None;
    for _ in 0..8 {
        let candidates = exact_text_bounds_in_semantics(robot, TARGET_TEXT);
        best_candidate_count = best_candidate_count.max(candidates.len());
        let screenshot = robot.screenshot_with_scale(CAPTURE_SCALE).ok()?;
        first_screenshot.get_or_insert_with(|| screenshot.clone());

        for bounds in candidates {
            first_candidate_bounds.get_or_insert(bounds);
            let center_y = bounds.1 + bounds.3 * 0.5;
            if !(80.0..=(WINDOW_HEIGHT as f32 - 40.0)).contains(&center_y) {
                skipped_out_of_view += 1;
                continue;
            }

            let region = text_capture_region(bounds);
            let output_size = region_output_size(region);
            let normalized =
                normalize_screenshot_region(&screenshot, region, output_size.0, output_size.1)?;
            let ink_pixels = normalized_region_ink_pixels(&normalized);
            if ink_pixels > best_ink {
                best_ink = ink_pixels;
                best_bounds = Some(bounds);
            }
            if ink_pixels >= MIN_NORMALIZED_TEXT_INK_PIXELS {
                return Some(TextRegionCapture {
                    bounds,
                    region,
                    screenshot,
                });
            }
        }

        std::thread::sleep(Duration::from_millis(60));
        let _ = robot.wait_for_idle();
    }

    println!(
        "  visible text capture failed: candidates_seen={best_candidate_count} skipped_out_of_view={skipped_out_of_view} first_candidate_bounds={first_candidate_bounds:?} best_ink={best_ink} best_bounds={best_bounds:?}"
    );
    if let Ok(semantics) = robot.get_semantics() {
        println!(
            "  semantic tree at capture failure:\n{}",
            cranpose::Robot::format_semantics(&semantics, 0)
        );
    }
    if let (Some(screenshot), Some(bounds)) = (first_screenshot.as_ref(), first_candidate_bounds) {
        let region = text_capture_region(bounds);
        let output_size = region_output_size(region);
        let direct_ink = direct_region_ink_pixels(screenshot, region);
        let best_red_row = best_red_decoration_row_in_screenshot(screenshot);
        println!(
            "  capture failure detail: screenshot={}x{} logical={:.1}x{:.1} scale=({:.3},{:.3}) region={region:?} output={output_size:?} direct_ink={direct_ink} best_red_row={best_red_row:?}",
            screenshot.width,
            screenshot.height,
            screenshot.logical_width,
            screenshot.logical_height,
            screenshot.width as f32 / screenshot.logical_width.max(1.0),
            screenshot.height as f32 / screenshot.logical_height.max(1.0),
        );
        if let Some(normalized) =
            normalize_screenshot_region(screenshot, region, output_size.0, output_size.1)
        {
            save_debug_images(
                "capture_failed",
                screenshot,
                screenshot,
                &normalized,
                &normalized,
            );
        }
    }
    None
}

fn exact_text_bounds_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
) -> Vec<(f32, f32, f32, f32)> {
    let Ok(roots) = robot.get_semantics() else {
        return Vec::new();
    };
    let mut bounds = Vec::new();
    for root in &roots {
        collect_matching_text_bounds(root, text, &mut bounds);
    }
    if let Some(bounds_from_helper) = find_text_in_semantics(robot, text) {
        if !bounds.contains(&bounds_from_helper) {
            bounds.push(bounds_from_helper);
        }
    }
    bounds
}

fn collect_matching_text_bounds(
    elem: &SemanticElement,
    text: &str,
    out: &mut Vec<(f32, f32, f32, f32)>,
) {
    if elem
        .text
        .as_deref()
        .is_some_and(|candidate| candidate.contains(text))
    {
        out.push((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
    }
    for child in &elem.children {
        collect_matching_text_bounds(child, text, out);
    }
}

fn normalized_region_ink_pixels(screenshot: &RobotScreenshot) -> usize {
    let Some(background) = screenshot.pixels.get(0..4) else {
        return 0;
    };
    screenshot
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| {
            let color_delta = pixel[0].abs_diff(background[0]) as u16
                + pixel[1].abs_diff(background[1]) as u16
                + pixel[2].abs_diff(background[2]) as u16;
            pixel[3] > 0 && color_delta > 24
        })
        .count()
}

fn direct_region_ink_pixels(screenshot: &RobotScreenshot, region: (f32, f32, f32, f32)) -> usize {
    let scale_x = screenshot.width as f32 / screenshot.logical_width.max(1.0);
    let scale_y = screenshot.height as f32 / screenshot.logical_height.max(1.0);
    let left = (region.0 * scale_x).floor().max(0.0) as usize;
    let top = (region.1 * scale_y).floor().max(0.0) as usize;
    let right = ((region.0 + region.2) * scale_x)
        .ceil()
        .min(screenshot.width as f32) as usize;
    let bottom = ((region.1 + region.3) * scale_y)
        .ceil()
        .min(screenshot.height as f32) as usize;
    let Some(background) = screenshot.pixels.get(0..4) else {
        return 0;
    };
    let mut count = 0usize;
    for y in top..bottom {
        for x in left..right {
            let index = (y * screenshot.width as usize + x) * 4;
            let Some(pixel) = screenshot.pixels.get(index..index + 4) else {
                continue;
            };
            let color_delta = pixel[0].abs_diff(background[0]) as u16
                + pixel[1].abs_diff(background[1]) as u16
                + pixel[2].abs_diff(background[2]) as u16;
            if pixel[3] > 0 && color_delta > 24 {
                count += 1;
            }
        }
    }
    count
}

fn best_red_decoration_row_in_screenshot(screenshot: &RobotScreenshot) -> Option<(usize, usize)> {
    let width = screenshot.width as usize;
    let height = screenshot.height as usize;
    let mut best = None;
    for y in 0..height {
        let mut row_red = 0usize;
        for x in 0..width {
            if is_red_decoration_pixel(screenshot, x, y) {
                row_red += 1;
            }
        }
        if best.is_none_or(|(_, best_count)| row_red > best_count) {
            best = Some((y, row_red));
        }
    }
    best.filter(|(_, count)| *count > 0)
}

fn text_capture_region(bounds: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let pad = 4.0;
    (
        bounds.0 - pad,
        bounds.1 - pad,
        bounds.2 + pad * 2.0,
        bounds.3 + pad * 2.0,
    )
}

fn region_output_size(region: (f32, f32, f32, f32)) -> (u32, u32) {
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
        robot
            .mouse_move(600.0, 450.0)
            .expect("move cursor to center");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll_and_wait_for_frame(0.0, -80.0)
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
    let output_dir = output_paths::diagnostic_path("cranpose_scroll_decoration_invariance");
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
