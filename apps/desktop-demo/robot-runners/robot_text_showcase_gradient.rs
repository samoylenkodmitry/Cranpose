//! Robot visual regression for Text showcase gradient/stroke sample.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_text_showcase_gradient --features robot-app`

use cranpose::AppLauncher;
use cranpose_testing::{
    crop_screenshot, find_button_exact_in_semantics, find_button_in_semantics, find_in_semantics,
    find_text,
};
use desktop_app::app;
use std::time::Duration;

fn fail(robot: &cranpose::Robot, message: &str) -> ! {
    println!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn main() {
    env_logger::init();
    println!("=== Robot Text Showcase Gradient ===");

    AppLauncher::new()
        .with_title("Robot Text Showcase Gradient")
        .with_size(1400, 1100)
        .with_fonts(&desktop_app::fonts::DEMO_FONTS)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            let Some((shaders_x, shaders_y, shaders_w, shaders_h)) =
                find_button_in_semantics(&robot, "Shaders")
            else {
                fail(&robot, "Shaders tab not found");
            };
            robot
                .click(shaders_x + shaders_w * 0.5, shaders_y + shaders_h * 0.5)
                .ok();
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();

            let Some((text_x, text_y, text_w, text_h)) =
                find_button_exact_in_semantics(&robot, "Text")
            else {
                fail(&robot, "Text tab not found after navigating to right-side tabs");
            };
            robot.click(text_x + text_w * 0.5, text_y + text_h * 0.5).ok();
            std::thread::sleep(Duration::from_millis(350));
            let _ = robot.wait_for_idle();

            if find_in_semantics(&robot, |elem| {
                find_text(elem, "Text Rendering Feature Showcase")
            })
            .is_none()
            {
                fail(&robot, "Text showcase heading not found after tab switch");
            }

            let Some((anchor_x, anchor_y, anchor_w, anchor_h)) = find_in_semantics(&robot, |elem| {
                find_text(elem, "brush + alpha + draw_style + platform_style")
            })
            else {
                fail(
                    &robot,
                    "gradient/stroke anchor label not found in Text showcase semantics",
                );
            };
            let sample_y = anchor_y + anchor_h + 2.0;
            let sample_w = anchor_w.max(280.0);
            let sample_h = (anchor_h * 2.2).max(28.0);

            let screenshot = robot
                .screenshot()
                .unwrap_or_else(|err| fail(&robot, &format!("screenshot failed: {err}")));

            let left = anchor_x.max(0.0).floor() as u32;
            let top = sample_y.max(0.0).floor() as u32;
            let right = (anchor_x + sample_w).max(0.0).ceil() as u32;
            let bottom = (sample_y + sample_h).max(0.0).ceil() as u32;
            let width = right.saturating_sub(left).max(1);
            let height = bottom.saturating_sub(top).max(1);
            let max_width = screenshot.width.saturating_sub(left);
            let max_height = screenshot.height.saturating_sub(top);
            let cropped = crop_screenshot(
                &screenshot,
                left,
                top,
                width.min(max_width).max(1),
                height.min(max_height).max(1),
            )
            .unwrap_or_else(|| fail(&robot, "failed to crop sample bounds"));

            let mut colored_pixels = 0usize;
            let mut min_red = 1.0f32;
            let mut max_red = 0.0f32;
            let mut min_blue = 1.0f32;
            let mut max_blue = 0.0f32;

            for rgba in cropped.pixels.chunks_exact(4) {
                let red = rgba[0] as f32 / 255.0;
                let green = rgba[1] as f32 / 255.0;
                let blue = rgba[2] as f32 / 255.0;
                let max_channel = red.max(green.max(blue));
                let min_channel = red.min(green.min(blue));
                let saturation = max_channel - min_channel;
                if max_channel > 0.30 && saturation > 0.08 {
                    colored_pixels += 1;
                    min_red = min_red.min(red);
                    max_red = max_red.max(red);
                    min_blue = min_blue.min(blue);
                    max_blue = max_blue.max(blue);
                }
            }

            if colored_pixels <= 20 {
                fail(
                    &robot,
                    &format!(
                        "expected visible colored ink in sample, got colored_pixels={colored_pixels}"
                    ),
                );
            }

            let red_span = max_red - min_red;
            let blue_span = max_blue - min_blue;
            let channel_span = red_span.max(blue_span);
            if channel_span <= 0.12 {
                fail(
                    &robot,
                    &format!(
                        "expected gradient variation in sample, red_span={red_span:.3}, blue_span={blue_span:.3}"
                    ),
                );
            }

            let Some((_p_x, _p_y, _p_w, p_h)) = find_in_semantics(&robot, |elem| {
                find_text(elem, "This paragraph demonstrates textIndent")
            }) else {
                fail(
                    &robot,
                    "paragraph sample text not found for multiline-wrap verification",
                );
            };
            if p_h <= 40.0 {
                fail(
                    &robot,
                    &format!(
                        "expected multiline paragraph sample height, got p_h={p_h:.1} (likely wrapped to a single clipped line)"
                    ),
                );
            }

            println!(
                "PASS: text showcase gradient/stroke sample has colored ink and variation (colored_pixels={colored_pixels}, red_span={red_span:.3}, blue_span={blue_span:.3}); paragraph wraps (height={p_h:.1})"
            );
            robot.exit().ok();
        })
        .run(app::combined_app);
}
