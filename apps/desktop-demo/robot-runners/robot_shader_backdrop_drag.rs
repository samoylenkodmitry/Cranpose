//! Robot test for backdrop-effect draggable overlays in the Shaders tab.
//!
//! Validates that "Blur" and "Glass" overlays produce visible pixel movement after drag.

use cranpose::AppLauncher;
use cranpose_testing::{capture_screenshot, find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn moved_enough(before: (f32, f32, f32, f32), after: (f32, f32, f32, f32), min_delta: f32) -> bool {
    let dx = (after.0 - before.0).abs();
    let dy = (after.1 - before.1).abs();
    dx >= min_delta || dy >= min_delta
}

fn changed_pixel_count(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    before
        .pixels
        .chunks_exact(4)
        .zip(after.pixels.chunks_exact(4))
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > channel_threshold
                || a[1].abs_diff(b[1]) > channel_threshold
                || a[2].abs_diff(b[2]) > channel_threshold
                || a[3].abs_diff(b[3]) > channel_threshold
        })
        .count()
}

fn changed_pixel_count_in_region(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    let left = region.0.max(0.0).floor() as u32;
    let top = region.1.max(0.0).floor() as u32;
    let right = (region.0 + region.2).min(before.width as f32).ceil() as u32;
    let bottom = (region.1 + region.3).min(before.height as f32).ceil() as u32;

    if right <= left || bottom <= top {
        return 0;
    }

    let width = before.width as usize;
    let mut changed = 0usize;

    for y in top..bottom {
        for x in left..right {
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

fn main() {
    env_logger::init();
    println!("=== Robot Shader Backdrop Drag Test ===");

    AppLauncher::new()
        .with_title("Robot Shader Backdrop Drag Test")
        .with_size(1200, 800)
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

            let Some(tab_before_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot before tab-row clip check");
                std::process::exit(1);
            };

            let _ = robot.drag(
                blur_scroll_start_x,
                blur_scroll_start_y,
                blur_scroll_start_x,
                tab_strip_bottom + 2.0,
            );
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();

            let Some(tab_after_shot) = capture_screenshot(&robot) else {
                println!("✗ Could not capture screenshot after tab-row clip check");
                std::process::exit(1);
            };

            let tab_strip_region = (
                0.0,
                (ty - 8.0).max(0.0),
                tab_before_shot.width as f32,
                th + 16.0,
            );
            let tab_strip_diff =
                changed_pixel_count_in_region(&tab_before_shot, &tab_after_shot, tab_strip_region, 10);
            println!(
                "Clip check: tab_strip_changed_pixels={} (region_bottom={:.1})",
                tab_strip_diff, tab_strip_bottom
            );

            if tab_strip_diff > 1_000 {
                println!(
                    "✗ Tab strip changed too much after dragging shader rect upward (changed_pixels={})",
                    tab_strip_diff
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
