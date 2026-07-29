//! Robot test for backdrop-effect draggable overlays in the Shaders tab.
//!
//! Validates that "Blur" and "Glass" overlays produce visible pixel movement after drag.

mod output_paths;
mod perf_contract;

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, changed_pixel_count, changed_pixel_count_in_region,
    find_button_in_semantics, find_text, find_text_by_prefix, find_text_by_prefix_in_semantics,
    find_text_in_semantics, parse_slider_value, root_bounds, screenshot_logical_size, y_is_visible,
};
use desktop_app::app;
use image::{ImageBuffer, RgbaImage};
use std::path::Path;
use std::time::Duration;

const EFFECT_SLIDER_WIDTH: f32 = 220.0;
const EFFECT_SLIDER_TOUCH_OFFSET_Y: f32 = 9.0;
const NESTED_SECTION_SCAN_ATTEMPTS: usize = 72;
const NESTED_SECTION_SCROLL_X: f32 = 620.0;
const NESTED_SECTION_SCROLL_Y: f32 = 500.0;
const NESTED_SECTION_SCROLL_STEP: f32 = 80.0;
const NESTED_SECTION_SEEK_STEP: f32 = 160.0;
const NESTED_SECTION_SCROLL_SETTLE_MS: u64 = 120;
const NESTED_BACKDROP_MIN_CHANGED_PIXELS: usize = 90;
const NESTED_BACKDROP_VISUAL_WAIT_ATTEMPTS: usize = 8;
const NESTED_BACKDROP_VISUAL_WAIT_MS: u64 = 80;
const MIN_EFFECT_DRAG_FRAMES: u64 = 6;
const EFFECT_DRAG_PRESENTED_STEPS: u32 = 36;
const MIN_EFFECT_DRAG_CADENCE_FPS: f32 = 120.0;
const MIN_EFFECT_DRAG_WORK_FPS: f32 = 120.0;
const MAX_EFFECT_DRAG_CADENCE_P95_MS: f32 = 8.33;
const MAX_EFFECT_DRAG_WORK_P95_MS: f32 = 8.33;

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn moved_enough(before: (f32, f32, f32, f32), after: (f32, f32, f32, f32), min_delta: f32) -> bool {
    let dx = (after.0 - before.0).abs();
    let dy = (after.1 - before.1).abs();
    dx >= min_delta || dy >= min_delta
}

fn scroll_up(robot: &cranpose::Robot) {
    cranpose_testing::scroll_up(robot, 620.0, 220.0, 720.0);
}

#[derive(Clone, Copy)]
struct NestedEffectControls {
    child_label: (f32, f32, f32, f32),
    parent_slider: (f32, f32, f32, f32),
    child_slider: (f32, f32, f32, f32),
}

fn visible_prefix(robot: &cranpose::Robot, prefix: &str) -> Option<(f32, f32, f32, f32, String)> {
    let bounds = find_text_by_prefix_in_semantics(robot, prefix)?;
    bounds_visible_in_shader_content(robot, (bounds.0, bounds.1, bounds.2, bounds.3))
        .then_some(bounds)
}

fn slider_touch_y(bounds: (f32, f32, f32, f32)) -> f32 {
    bounds.1 + bounds.3 + EFFECT_SLIDER_TOUCH_OFFSET_Y
}

fn shader_content_top(robot: &cranpose::Robot) -> f32 {
    find_button_in_semantics(robot, "Shaders")
        .map(|(_, y, _, h)| y + h + 16.0)
        .or_else(|| root_bounds(robot).map(|(_, y, _, _)| y + 96.0))
        .unwrap_or(96.0)
}

fn shader_content_bottom(robot: &cranpose::Robot) -> f32 {
    root_bounds(robot)
        .map(|(_, y, _, h)| y + h - 28.0)
        .unwrap_or(f32::MAX)
}

fn y_in_shader_content(robot: &cranpose::Robot, y: f32) -> bool {
    y_is_visible(robot, y) && y >= shader_content_top(robot) && y <= shader_content_bottom(robot)
}

fn bounds_visible_in_shader_content(robot: &cranpose::Robot, bounds: (f32, f32, f32, f32)) -> bool {
    y_in_shader_content(robot, bounds.1)
        && y_in_shader_content(robot, bounds.1 + bounds.3 * 0.5)
        && y_in_shader_content(robot, bounds.1 + bounds.3)
}

fn visible_nested_effect_controls(robot: &cranpose::Robot) -> Option<NestedEffectControls> {
    let semantics = robot.get_semantics().ok()?;
    let child_label = visible_text_in_snapshot(robot, &semantics, "Child backdrop")?;
    let (parent_x, parent_y, parent_w, parent_h, _) =
        visible_slider_prefix_in_snapshot(robot, &semantics, "nested_parent_blur")?;
    let (child_x, child_y, child_w, child_h, _) =
        visible_slider_prefix_in_snapshot(robot, &semantics, "nested_child_backdrop_blur")?;
    Some(NestedEffectControls {
        child_label,
        parent_slider: (parent_x, parent_y, parent_w, parent_h),
        child_slider: (child_x, child_y, child_w, child_h),
    })
}

fn visible_text_in_snapshot(
    robot: &cranpose::Robot,
    semantics: &[cranpose::SemanticElement],
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    semantics
        .iter()
        .find_map(|root| find_text(root, text))
        .filter(|bounds| bounds_visible_in_shader_content(robot, *bounds))
}

fn visible_slider_prefix_in_snapshot(
    robot: &cranpose::Robot,
    semantics: &[cranpose::SemanticElement],
    prefix: &str,
) -> Option<(f32, f32, f32, f32, String)> {
    let bounds = semantics
        .iter()
        .find_map(|root| find_text_by_prefix(root, prefix))?;
    let touch_y = slider_touch_y((bounds.0, bounds.1, bounds.2, bounds.3));
    (bounds_visible_in_shader_content(robot, (bounds.0, bounds.1, bounds.2, bounds.3))
        && y_in_shader_content(robot, touch_y))
    .then_some(bounds)
}

fn scroll_nested_effect_controls_into_view(
    robot: &cranpose::Robot,
) -> Option<NestedEffectControls> {
    for _ in 0..NESTED_SECTION_SCAN_ATTEMPTS {
        if let Some(controls) = visible_nested_effect_controls(robot) {
            return Some(controls);
        }
        if let Err(error) = nudge_nested_effect_controls_toward_view(robot) {
            println!("Nested controls scroll failed: {error}");
            return None;
        }
    }
    None
}

fn nested_effect_controls_vertical_span(robot: &cranpose::Robot) -> Option<(f32, f32)> {
    let child_label = find_text_in_semantics(robot, "Child backdrop");
    let parent_slider = find_text_by_prefix_in_semantics(robot, "nested_parent_blur")
        .map(|(x, y, w, h, _)| (x, y, w, h));
    let child_slider = find_text_by_prefix_in_semantics(robot, "nested_child_backdrop_blur")
        .map(|(x, y, w, h, _)| (x, y, w, h));

    let top = [child_label, parent_slider, child_slider]
        .into_iter()
        .flatten()
        .map(|(_, y, _, _)| y)
        .reduce(f32::min)?;
    let bottom = [
        child_label.map(|(_, y, _, h)| y + h),
        parent_slider.map(slider_touch_y),
        child_slider.map(slider_touch_y),
    ]
    .into_iter()
    .flatten()
    .reduce(f32::max)?;
    Some((top, bottom))
}

fn nudge_nested_effect_controls_toward_view(robot: &cranpose::Robot) -> Result<(), String> {
    let top = shader_content_top(robot);
    let bottom = shader_content_bottom(robot);
    let delta_y = match nested_effect_controls_vertical_span(robot) {
        Some((target_top, _)) if target_top < top => NESTED_SECTION_SCROLL_STEP,
        Some((_, target_bottom)) if target_bottom > bottom => -NESTED_SECTION_SCROLL_STEP,
        Some((target_top, target_bottom)) => {
            let target_center = (target_top + target_bottom) * 0.5;
            let viewport_center = (top + bottom) * 0.5;
            if target_center < viewport_center {
                NESTED_SECTION_SCROLL_STEP
            } else {
                -NESTED_SECTION_SCROLL_STEP
            }
        }
        None => -NESTED_SECTION_SEEK_STEP,
    };

    robot.mouse_move(NESTED_SECTION_SCROLL_X, NESTED_SECTION_SCROLL_Y)?;
    robot.mouse_scroll(0.0, delta_y)?;
    std::thread::sleep(Duration::from_millis(NESTED_SECTION_SCROLL_SETTLE_MS));
    robot.wait_for_idle()
}

fn log_nested_effect_probe(robot: &cranpose::Robot) {
    let child_label = find_text_in_semantics(robot, "Child backdrop");
    let parent_slider = find_text_by_prefix_in_semantics(robot, "nested_parent_blur");
    let child_slider = find_text_by_prefix_in_semantics(robot, "nested_child_backdrop_blur");
    println!(
        "Nested probe: child_label={:?} parent_slider={:?} child_slider={:?}",
        child_label, parent_slider, child_slider
    );
}

fn save_png(path: &Path, screenshot: &cranpose::RobotScreenshot) -> Result<(), String> {
    let image: RgbaImage = ImageBuffer::from_raw(
        screenshot.width,
        screenshot.height,
        screenshot.pixels.clone(),
    )
    .ok_or_else(|| "invalid screenshot dimensions".to_string())?;
    image
        .save(path)
        .map_err(|err| format!("failed to save {}: {err}", path.display()))
}

fn save_crop_png(
    path: &Path,
    screenshot: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
) -> Result<(), String> {
    let x0 = region.0.max(0.0).floor() as u32;
    let y0 = region.1.max(0.0).floor() as u32;
    let x1 = (region.0 + region.2)
        .min(screenshot.width as f32)
        .ceil()
        .max(x0 as f32) as u32;
    let y1 = (region.1 + region.3)
        .min(screenshot.height as f32)
        .ceil()
        .max(y0 as f32) as u32;
    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    let stride = screenshot.width as usize * 4;
    for y in y0..y1 {
        let start = y as usize * stride + x0 as usize * 4;
        let end = start + width as usize * 4;
        pixels.extend_from_slice(&screenshot.pixels[start..end]);
    }
    let image: RgbaImage =
        ImageBuffer::from_raw(width, height, pixels).ok_or_else(|| "invalid crop".to_string())?;
    image
        .save(path)
        .map_err(|err| format!("failed to save {}: {err}", path.display()))
}

fn save_nested_backdrop_diagnostic(
    label: &str,
    screenshot: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
) {
    let full_path =
        output_paths::diagnostic_path(&format!("shader_nested_backdrop_{label}_full.png"));
    let crop_path =
        output_paths::diagnostic_path(&format!("shader_nested_backdrop_{label}_crop.png"));
    if let Err(err) = save_png(&full_path, screenshot) {
        println!("Nested diagnostic save failed: {err}");
    }
    if let Err(err) = save_crop_png(&crop_path, screenshot, region) {
        println!("Nested diagnostic crop save failed: {err}");
    }
    println!(
        "Nested diagnostic {label}: full={} crop={} region=({:.1},{:.1},{:.1},{:.1}) screenshot={}x{}",
        full_path.display(),
        crop_path.display(),
        region.0,
        region.1,
        region.2,
        region.3,
        screenshot.width,
        screenshot.height
    );
}

fn set_visible_slider_fraction(
    robot: &cranpose::Robot,
    slider_bounds: (f32, f32, f32, f32),
    prefix: &str,
    fraction: f32,
) -> Option<f32> {
    let slider_y = slider_bounds.1 + slider_bounds.3 + EFFECT_SLIDER_TOUCH_OFFSET_Y;
    let left_x = slider_bounds.0 + 2.0;
    let target_x = slider_bounds.0 + EFFECT_SLIDER_WIDTH * fraction.clamp(0.0, 1.0);
    let _ = robot.drag(left_x, slider_y, target_x, slider_y);
    std::thread::sleep(Duration::from_millis(120));
    let _ = robot.wait_for_idle();
    visible_prefix(robot, prefix).and_then(|(_, _, _, _, text)| parse_slider_value(&text))
}

fn wait_for_nested_backdrop_region_change(
    robot: &cranpose::Robot,
    baseline: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    baseline_noise: usize,
) -> Option<(usize, usize)> {
    let mut last = None;
    for attempt in 1..=NESTED_BACKDROP_VISUAL_WAIT_ATTEMPTS {
        if attempt > 1 {
            std::thread::sleep(Duration::from_millis(NESTED_BACKDROP_VISUAL_WAIT_MS));
            let _ = robot.wait_for_idle();
        }
        let screenshot = capture_screenshot(robot)?;
        if attempt == 1 {
            save_nested_backdrop_diagnostic("after", &screenshot, region);
        }
        let raw = changed_pixel_count_in_region(baseline, &screenshot, region, 10);
        let net = raw.saturating_sub(baseline_noise);
        if let Ok(Some(stats)) = robot.get_render_stats() {
            println!(
                "Nested child backdrop diff attempt {attempt}: raw={raw} baseline={baseline_noise} net={net} blur={} composite={} layer_cache_hit={} layer_cache_miss={} isolated={} pass={} submit={}",
                stats.blur_passes,
                stats.composite_passes,
                stats.layer_cache_hits,
                stats.layer_cache_misses,
                stats.isolated_layer_renders,
                stats.pass_count,
                stats.submit_count
            );
        } else {
            println!(
                "Nested child backdrop diff attempt {attempt}: raw={raw} baseline={baseline_noise} net={net}"
            );
        }
        last = Some((raw, net));
        if net >= NESTED_BACKDROP_MIN_CHANGED_PIXELS {
            break;
        }
    }
    last
}

fn assert_effect_drag_performance(
    robot: &cranpose::Robot,
    label: &str,
    render_stats: Option<cranpose_render_wgpu::RenderStatsSnapshot>,
) {
    let stats = robot
        .fps_stats()
        .unwrap_or_else(|err| panic!("{label}: failed to read FPS stats: {err}"));
    println!(
        "{label} fps_summary: cadence_fps={:.1} work_fps={:.1} p95_ms={:.2} work_p95_ms={:.2} work_max_ms={:.2} missed_120hz={} work_missed_120hz={} stalls_50ms={} work_stalls_50ms={} frames={}",
        stats.fps,
        stats.work_fps,
        stats.p95_ms,
        stats.work_p95_ms,
        stats.work_max_ms,
        stats.missed_120hz_budget,
        stats.work_missed_120hz_budget,
        stats.stalled_50ms_frames,
        stats.work_stalled_50ms_frames,
        stats.frame_count
    );
    if let Some(render_stats) = render_stats {
        println!(
            "{label} render_stats: submit={} encoder={} pass={} blur={} composite={} effect={} isolated={} layer_hit={} layer_miss={} transient_bytes={} retained_bytes={} upload_bytes={}",
            render_stats.submit_count,
            render_stats.encoder_count,
            render_stats.pass_count,
            render_stats.blur_passes,
            render_stats.composite_passes,
            render_stats.effect_applies,
            render_stats.isolated_layer_renders,
            render_stats.layer_cache_hits,
            render_stats.layer_cache_misses,
            render_stats.transient_texture_bytes,
            render_stats.retained_texture_bytes,
            render_stats.upload_bytes
        );
    }

    assert!(
        stats.frame_count >= MIN_EFFECT_DRAG_FRAMES,
        "{label}: drag did not produce enough measured frames: {stats:?}"
    );
    if perf_contract::hardware_performance_contracts_enabled() {
        assert!(
            stats.fps >= MIN_EFFECT_DRAG_CADENCE_FPS
                && stats.p95_ms <= MAX_EFFECT_DRAG_CADENCE_P95_MS
                && stats.stalled_50ms_frames == 0
                && stats.work_fps >= MIN_EFFECT_DRAG_WORK_FPS
                && stats.work_p95_ms <= MAX_EFFECT_DRAG_WORK_P95_MS
                && stats.work_stalled_50ms_frames == 0,
            "{label}: shader drag missed the 120Hz presented-frame contract: {stats:?}"
        );
    } else {
        perf_contract::log_software_renderer_budget(label);
    }

    if let Some(render_stats) = render_stats {
        assert_eq!(
            render_stats.submit_count, 1,
            "{label}: presented shader drag frame must submit once before screenshot readback: {render_stats:?}"
        );
        assert_eq!(
            render_stats.encoder_count, 1,
            "{label}: presented shader drag frame must use one command encoder before screenshot readback: {render_stats:?}"
        );
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Shader Backdrop Drag Test ===");

    AppLauncher::new()
        .with_title("Robot Shader Backdrop Drag Test")
        .with_size(1200, 1000)
        .with_headless(false)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let Some((tx, ty, tw, th)) = find_button_in_semantics(&robot, "Shaders") else {
                println!("✗ Could not find 'Shaders' tab");
                std::process::exit(1);
            };
            let tab_strip_bottom = ty + th + 12.0;
            let _ = robot.click(tx + tw * 0.5, ty + th * 0.5);
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            // Wait for shader screen content to appear.
            let mut ready = false;
            for _ in 0..20 {
                if find_text_in_semantics(&robot, "Interactive Effects (drag the rects!)").is_some() {
                    ready = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if !ready {
                println!("✗ Shaders tab content did not appear");
                std::process::exit(1);
            }

            if find_text_in_semantics(&robot, "Effect Semantics Checks").is_none() {
                println!("✗ Missing 'Effect Semantics Checks' demo block");
                std::process::exit(1);
            }
            if find_text_in_semantics(&robot, "Blur Decal").is_none() {
                println!("✗ Missing 'Blur Decal' semantics demo card");
                std::process::exit(1);
            }
            if find_text_in_semantics(&robot, "Cut / Opacity Mask APIs").is_none() {
                println!("✗ Missing 'Cut / Opacity Mask APIs' demo block");
                std::process::exit(1);
            }
            if find_text_in_semantics(&robot, "Half screen cut").is_none() {
                println!("✗ Missing 'Half screen cut' mask preview card");
                std::process::exit(1);
            }
            if find_text_in_semantics(&robot, "DstOut vertical fade").is_none() {
                println!("✗ Missing 'DstOut vertical fade' mask preview card");
                std::process::exit(1);
            }

            let Some(nested_controls) = scroll_nested_effect_controls_into_view(&robot) else {
                log_nested_effect_probe(&robot);
                println!(
                    "✗ Could not bring nested backdrop preview and sliders into view together"
                );
                std::process::exit(1);
            };

            // Regression: nested child backdrop blur must visibly affect pixels.
            let nested_parent = set_visible_slider_fraction(
                &robot,
                nested_controls.parent_slider,
                "nested_parent_blur",
                0.0,
            );
            let nested_child_off = set_visible_slider_fraction(
                &robot,
                nested_controls.child_slider,
                "nested_child_backdrop_blur",
                0.0,
            );
            println!(
                "Nested sliders (off): parent={:?} child={:?}",
                nested_parent, nested_child_off
            );
            if nested_parent.is_none_or(|value| value > 1.0)
                || nested_child_off.is_none_or(|value| value > 1.0)
            {
                println!("✗ Could not set nested backdrop sliders to baseline values");
                std::process::exit(1);
            }

            let Some(nested_controls) = visible_nested_effect_controls(&robot) else {
                println!("✗ Nested backdrop controls moved out of view after slider setup");
                std::process::exit(1);
            };
            let (label_x, label_y, label_w, label_h) = nested_controls.child_label;
            let nested_region = (
                (label_x - 56.0).max(0.0),
                (label_y - 30.0).max(0.0),
                label_w + 112.0,
                label_h + 62.0,
            );

            let Some(nested_base_a) = capture_screenshot(&robot) else {
                println!("✗ Could not capture nested backdrop baseline screenshot A");
                std::process::exit(1);
            };
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();
            let Some(nested_base_b) = capture_screenshot(&robot) else {
                println!("✗ Could not capture nested backdrop baseline screenshot B");
                std::process::exit(1);
            };
            let nested_baseline_noise = changed_pixel_count_in_region(
                &nested_base_a,
                &nested_base_b,
                nested_region,
                10,
            );
            save_nested_backdrop_diagnostic("baseline", &nested_base_b, nested_region);

            let nested_child_on = set_visible_slider_fraction(
                &robot,
                nested_controls.child_slider,
                "nested_child_backdrop_blur",
                1.0,
            );
            println!("Nested child slider (on): {:?}", nested_child_on);
            if nested_child_on.is_none_or(|value| value < 16.0) {
                println!("✗ Could not set nested_child_backdrop_blur to maximum value");
                std::process::exit(1);
            }
            let _ = robot.click(24.0, 24.0);
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.wait_for_idle();

            let Some((nested_raw, nested_net)) = wait_for_nested_backdrop_region_change(
                &robot,
                &nested_base_b,
                nested_region,
                nested_baseline_noise,
            ) else {
                println!("✗ Could not capture nested backdrop screenshot after slider");
                std::process::exit(1);
            };
            println!(
                "Nested child backdrop diff: raw={} baseline={} net={}",
                nested_raw, nested_baseline_noise, nested_net
            );
            if nested_net < NESTED_BACKDROP_MIN_CHANGED_PIXELS {
                println!(
                    "✗ nested_child_backdrop_blur did not produce visible changes (net={})",
                    nested_net
                );
                std::process::exit(1);
            }

            // Return to the upper area for the draggable Blur/Glass regression checks.
            for _ in 0..4 {
                scroll_up(&robot);
            }

            let Some(blur_before) = find_text_in_semantics(&robot, "Blur") else {
                println!("✗ Could not find 'Blur' label");
                std::process::exit(1);
            };
            let Some(glass_before) = find_text_in_semantics(&robot, "Glass") else {
                println!("✗ Could not find 'Glass' label");
                std::process::exit(1);
            };
            let (blur_cx, blur_cy) = center(blur_before);
            let (glass_cx, glass_cy) = center(glass_before);
            println!(
                "Initial semantic label positions: Blur=({:.1},{:.1}) Glass=({:.1},{:.1})",
                blur_before.0,
                blur_before.1,
                glass_before.0,
                glass_before.1
            );

            // Semantic bounds do not include graphics-layer translation yet; offset by
            // the known initial state used by InteractiveEffectsDemo to hit the visual rect.
            let blur_start_x = blur_cx + 16.0;
            let blur_start_y = blur_cy + 16.0;

            let Some(blur_before_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot before blur drag");
                std::process::exit(1);
            };

            robot.reset_fps_stats().expect("reset blur drag FPS stats");
            let _ = robot.drag_and_wait_for_frames(
                blur_start_x,
                blur_start_y,
                blur_start_x + 90.0,
                blur_start_y + 70.0,
                EFFECT_DRAG_PRESENTED_STEPS,
            );
            let blur_render_stats = robot.get_render_stats().ok().flatten();
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            let Some(blur_after) = find_text_in_semantics(&robot, "Blur") else {
                println!("✗ Lost 'Blur' label after drag");
                std::process::exit(1);
            };
            let Some(blur_after_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot after blur drag");
                std::process::exit(1);
            };
            let blur_pixel_diff = changed_pixel_count(&blur_before_shot, &blur_after_shot, 10);
            println!(
                "After blur drag: semantic Blur=({:.1},{:.1}) changed_pixels={}",
                blur_after.0, blur_after.1, blur_pixel_diff
            );

            if blur_pixel_diff < 2_500 && !moved_enough(blur_before, blur_after, 20.0) {
                println!(
                    "✗ Blur did not produce visible movement after drag (before=({:.1},{:.1}) after=({:.1},{:.1}) pixels={})",
                    blur_before.0, blur_before.1, blur_after.0, blur_after.1, blur_pixel_diff
                );
                std::process::exit(1);
            }
            assert_effect_drag_performance(&robot, "Blur drag", blur_render_stats);

            // Glass starts at (244,164) in local area coordinates.
            let glass_start_x = glass_cx + 244.0;
            let glass_start_y = glass_cy + 164.0;

            let Some(glass_before_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot before glass drag");
                std::process::exit(1);
            };

            robot.reset_fps_stats().expect("reset glass drag FPS stats");
            let _ = robot.drag_and_wait_for_frames(
                glass_start_x,
                glass_start_y,
                glass_start_x - 100.0,
                glass_start_y - 60.0,
                EFFECT_DRAG_PRESENTED_STEPS,
            );
            let glass_render_stats = robot.get_render_stats().ok().flatten();
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            let Some(glass_after) = find_text_in_semantics(&robot, "Glass") else {
                println!("✗ Lost 'Glass' label after drag");
                std::process::exit(1);
            };
            let Some(glass_after_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot after glass drag");
                std::process::exit(1);
            };
            let glass_pixel_diff = changed_pixel_count(&glass_before_shot, &glass_after_shot, 10);
            println!(
                "After glass drag: semantic Glass=({:.1},{:.1}) changed_pixels={}",
                glass_after.0, glass_after.1, glass_pixel_diff
            );

            if glass_pixel_diff < 2_500 && !moved_enough(glass_before, glass_after, 20.0) {
                println!(
                    "✗ Glass did not produce visible movement after drag (before=({:.1},{:.1}) after=({:.1},{:.1}) pixels={})",
                    glass_before.0, glass_before.1, glass_after.0, glass_after.1, glass_pixel_diff
                );
                std::process::exit(1);
            }
            assert_effect_drag_performance(&robot, "Glass drag", glass_render_stats);

            // Regression check for clipping against the tab row:
            // 1) Scroll the shader content
            // 2) Drag Blur rect up toward the top edge of its clipped area
            // 3) Verify tab strip pixels do not change significantly
            let _ = robot.drag(610.0, 670.0, 610.0, 220.0);
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();

            let Some(blur_scrolled) = find_text_in_semantics(&robot, "Blur") else {
                println!("✗ Could not find 'Blur' label after scrolling for clip test");
                std::process::exit(1);
            };
            let (blur_scrolled_cx, blur_scrolled_cy) = center(blur_scrolled);
            let blur_scroll_start_x = blur_scrolled_cx + 16.0;
            let blur_scroll_start_y = blur_scrolled_cy + 16.0;

            let Some(tab_noise_before) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot for tab-row baseline (before)");
                std::process::exit(1);
            };
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();
            let Some(tab_noise_after) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot for tab-row baseline (after)");
                std::process::exit(1);
            };
            let (logical_width, logical_height) = screenshot_logical_size(&tab_noise_before);

            let tab_strip_region = (
                0.0,
                (ty - 8.0).max(0.0),
                logical_width,
                th + 16.0,
            );
            let baseline_tab_strip_diff = changed_pixel_count_in_region(
                &tab_noise_before,
                &tab_noise_after,
                tab_strip_region,
                10,
            );
            let _ = robot.drag(
                blur_scroll_start_x,
                blur_scroll_start_y,
                blur_scroll_start_x,
                tab_strip_bottom + 2.0,
            );
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();

            // Move pointer away from the tab strip before taking the "after"
            // screenshot to avoid hover-highlight false positives.
            let safe_x = blur_scroll_start_x.clamp(32.0, logical_width - 32.0);
            let safe_y =
                (tab_strip_bottom + 120.0).clamp(tab_strip_bottom + 20.0, logical_height - 32.0);
            let _ = robot.click(safe_x, safe_y);
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();

            let Some(tab_after_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot after tab-row clip check");
                std::process::exit(1);
            };

            let raw_tab_strip_diff =
                changed_pixel_count_in_region(&tab_noise_after, &tab_after_shot, tab_strip_region, 10);
            let tab_strip_diff = raw_tab_strip_diff.saturating_sub(baseline_tab_strip_diff);
            println!(
                "Clip check: tab_strip_changed_pixels={} (raw={} baseline={} region_bottom={:.1})",
                tab_strip_diff, raw_tab_strip_diff, baseline_tab_strip_diff, tab_strip_bottom
            );

            // In headless WGPU runs we still observe non-deterministic tab-strip deltas
            // from gesture-side effects (hover/scroll feedback). Keep this threshold
            // high enough to avoid flakiness while still catching obvious overflow.
            if tab_strip_diff > 15_000 {
                println!(
                    "✗ Tab strip changed too much after dragging shader rect upward (delta={} raw={} baseline={})",
                    tab_strip_diff, raw_tab_strip_diff, baseline_tab_strip_diff
                );
                std::process::exit(1);
            }

            println!("✓ PASS: Blur/Glass drag and tab-row clipping checks passed");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
