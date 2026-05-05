//! Robot test for backdrop-effect draggable overlays in the Shaders tab.
//!
//! Validates that "Blur" and "Glass" overlays produce visible pixel movement after drag.

use cranpose::AppLauncher;
use cranpose_testing::{
    capture_screenshot, changed_pixel_count, changed_pixel_count_in_region,
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
    parse_slider_value, root_bounds, screenshot_logical_size, y_is_visible,
};
use desktop_app::app;
use std::time::Duration;

const EFFECT_SLIDER_WIDTH: f32 = 220.0;
const EFFECT_SLIDER_TOUCH_OFFSET_Y: f32 = 9.0;
const NESTED_SECTION_SCAN_ATTEMPTS: usize = 72;
const NESTED_SECTION_RESET_DRAGS: usize = 8;

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

fn scroll_down_small(robot: &cranpose::Robot) {
    cranpose_testing::scroll_down(robot, 620.0, 680.0, 560.0);
}

fn scroll_up_small(robot: &cranpose::Robot) {
    cranpose_testing::scroll_up(robot, 620.0, 560.0, 680.0);
}

#[derive(Clone, Copy)]
struct NestedEffectControls {
    child_label: (f32, f32, f32, f32),
    parent_slider: (f32, f32, f32, f32),
    child_slider: (f32, f32, f32, f32),
}

fn visible_text(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    let bounds = find_text_in_semantics(robot, text)?;
    let center_y = bounds.1 + bounds.3 * 0.5;
    y_is_visible(robot, center_y).then_some(bounds)
}

fn visible_prefix(robot: &cranpose::Robot, prefix: &str) -> Option<(f32, f32, f32, f32, String)> {
    let bounds = find_text_by_prefix_in_semantics(robot, prefix)?;
    let center_y = bounds.1 + bounds.3 * 0.5;
    y_is_visible(robot, center_y).then_some(bounds)
}

fn slider_touch_y(bounds: (f32, f32, f32, f32)) -> f32 {
    bounds.1 + bounds.3 + EFFECT_SLIDER_TOUCH_OFFSET_Y
}

fn visible_slider_prefix(
    robot: &cranpose::Robot,
    prefix: &str,
) -> Option<(f32, f32, f32, f32, String)> {
    let bounds = find_text_by_prefix_in_semantics(robot, prefix)?;
    let label_center_y = bounds.1 + bounds.3 * 0.5;
    let touch_y = slider_touch_y((bounds.0, bounds.1, bounds.2, bounds.3));
    (y_is_visible(robot, label_center_y) && y_is_visible(robot, touch_y)).then_some(bounds)
}

fn visible_nested_effect_controls(robot: &cranpose::Robot) -> Option<NestedEffectControls> {
    let child_label = visible_text(robot, "Child backdrop")?;
    let (parent_x, parent_y, parent_w, parent_h, _) =
        visible_slider_prefix(robot, "nested_parent_blur")?;
    let (child_x, child_y, child_w, child_h, _) =
        visible_slider_prefix(robot, "nested_child_backdrop_blur")?;
    Some(NestedEffectControls {
        child_label,
        parent_slider: (parent_x, parent_y, parent_w, parent_h),
        child_slider: (child_x, child_y, child_w, child_h),
    })
}

fn scroll_nested_effect_controls_into_view(
    robot: &cranpose::Robot,
) -> Option<NestedEffectControls> {
    for _ in 0..NESTED_SECTION_RESET_DRAGS {
        scroll_up(robot);
    }
    for _ in 0..NESTED_SECTION_SCAN_ATTEMPTS {
        if let Some(controls) = visible_nested_effect_controls(robot) {
            return Some(controls);
        }
        nudge_nested_effect_controls_toward_view(robot);
    }
    None
}

fn nudge_nested_effect_controls_toward_view(robot: &cranpose::Robot) {
    let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
        scroll_down_small(robot);
        return;
    };
    let top = root_y + 96.0;
    let bottom = root_y + root_h - 96.0;
    let raw_bounds = [
        find_text_in_semantics(robot, "Child backdrop"),
        find_text_by_prefix_in_semantics(robot, "nested_parent_blur")
            .map(|(x, y, w, h, _)| (x, y, w, h)),
        find_text_by_prefix_in_semantics(robot, "nested_child_backdrop_blur")
            .map(|(x, y, w, h, _)| (x, y, w, h)),
    ];

    let target_above = raw_bounds
        .iter()
        .flatten()
        .any(|(_, y, _, h)| y + h * 0.5 < top);
    let target_below = raw_bounds
        .iter()
        .flatten()
        .any(|(_, y, _, h)| y + h * 0.5 > bottom)
        || [raw_bounds[1], raw_bounds[2]]
            .into_iter()
            .flatten()
            .any(|bounds| slider_touch_y(bounds) > bottom);

    if target_above {
        scroll_up_small(robot);
    } else if target_below {
        scroll_down_small(robot);
    } else {
        scroll_down_small(robot);
    }
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

fn main() {
    env_logger::init();
    println!("=== Robot Shader Backdrop Drag Test ===");

    AppLauncher::new()
        .with_title("Robot Shader Backdrop Drag Test")
        .with_size(1200, 1000)
        .with_headless(true)
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

            let Some(mut nested_controls) = scroll_nested_effect_controls_into_view(&robot) else {
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
            nested_controls = visible_nested_effect_controls(&robot).unwrap_or(nested_controls);
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

            let Some(nested_after) = capture_screenshot(&robot) else {
                println!("✗ Could not capture nested backdrop screenshot after slider");
                std::process::exit(1);
            };
            let nested_raw = changed_pixel_count_in_region(
                &nested_base_b,
                &nested_after,
                nested_region,
                10,
            );
            let nested_net = nested_raw.saturating_sub(nested_baseline_noise);
            println!(
                "Nested child backdrop diff: raw={} baseline={} net={}",
                nested_raw, nested_baseline_noise, nested_net
            );
            if nested_net < 90 {
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

            let _ = robot.drag(
                blur_start_x,
                blur_start_y,
                blur_start_x + 90.0,
                blur_start_y + 70.0,
            );
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

            // Glass starts at (244,164) in local area coordinates.
            let glass_start_x = glass_cx + 244.0;
            let glass_start_y = glass_cy + 164.0;

            let Some(glass_before_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot before glass drag");
                std::process::exit(1);
            };

            let _ = robot.drag(
                glass_start_x,
                glass_start_y,
                glass_start_x - 100.0,
                glass_start_y - 60.0,
            );
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
