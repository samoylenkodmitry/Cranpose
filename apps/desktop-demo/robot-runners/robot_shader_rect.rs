//! Robot test for the Shader Rect tab — validates both the SDF halo border
//! and fire shader effects render correctly.
//!
//! Run with:
//! `cargo run --package desktop-app --example robot_shader_rect --features robot-app`

use cranpose::AppLauncher;
use cranpose_testing::{
    find_button_in_semantics, find_text_in_semantics, sample_screenshot_pixel_logical,
};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Shader Rect Test ===");

    AppLauncher::new()
        .with_title("Robot Shader Rect")
        .with_size(1024, 900)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(600));
            let _ = robot.wait_for_idle();

            // Navigate to Shader Rect tab
            let Some((tab_x, tab_y, tab_w, tab_h)) =
                find_button_in_semantics(&robot, "Shader Rect")
            else {
                println!("FATAL: Shader Rect tab button not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            robot.click(tab_x + tab_w / 2.0, tab_y + tab_h / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Verify tab content loaded
            if find_text_in_semantics(&robot, "SDF Halo Border").is_none() {
                println!("FATAL: Tab header text not found");
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("OK: Tab loaded, header text found");

            // ========== Halo border test ==========
            let Some((box1_x, box1_y, box1_w, box1_h)) = find_text_in_semantics(&robot, "Box 1")
            else {
                println!("FATAL: Box 1 text not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!(
                "OK: Box 1 at ({:.1}, {:.1}, {:.1}x{:.1})",
                box1_x, box1_y, box1_w, box1_h
            );

            let center_x = box1_x + box1_w / 2.0;
            let center_y = box1_y + box1_h / 2.0;
            let content_right = center_x + 140.0;

            let screenshot = match robot.screenshot() {
                Ok(s) => s,
                Err(e) => {
                    println!("FATAL: screenshot failed: {e}");
                    robot.exit().ok();
                    std::process::exit(1);
                }
            };

            // Verify halo is visible outside content rect
            let mut halo_found = false;
            for &offset in &[4.0, 8.0, 16.0] {
                let sx = content_right + offset;
                if let Some(pixel) = sample_screenshot_pixel_logical(&screenshot, sx, center_y) {
                    println!(
                        "  halo right +{:.0}dp: rgba({}, {}, {}, {})",
                        offset, pixel[0], pixel[1], pixel[2], pixel[3]
                    );
                    if pixel[2] > 30 && pixel[2] > pixel[0] + 10 {
                        halo_found = true;
                    }
                }
            }

            if !halo_found {
                println!("FATAL: No blue halo pixels detected outside Box 1");
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("OK: Halo glow detected");

            // ========== Fire shader test ==========
            // Wait a frame for the fire animation to produce some output
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            // The fire shader box should be visible (may need to scroll)
            let fire_text = find_text_in_semantics(&robot, "Fire");
            if fire_text.is_none() {
                // Try scrolling down
                robot.mouse_scroll(0.0, -300.0).ok();
                std::thread::sleep(Duration::from_millis(400));
                let _ = robot.wait_for_idle();
            }

            let screenshot2 = match robot.screenshot() {
                Ok(s) => s,
                Err(e) => {
                    println!("FATAL: screenshot2 failed: {e}");
                    robot.exit().ok();
                    std::process::exit(1);
                }
            };

            let Some((fire_x, fire_y, fire_w, fire_h)) = find_text_in_semantics(&robot, "Fire")
            else {
                println!("FATAL: Fire text not found (even after scroll)");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!(
                "OK: Fire box at ({:.1}, {:.1}, {:.1}x{:.1})",
                fire_x, fire_y, fire_w, fire_h
            );

            let fire_cx = fire_x + fire_w / 2.0;
            let fire_cy = fire_y + fire_h / 2.0;
            // Content is 280x140, padding is 48, so content edge is ~140dp from center
            let fire_content_right = fire_cx + 140.0;

            // Sample the fire halo region — flame pixels should have warm colors
            // (high red, some green, lower blue — fire ramp produces magenta/orange/yellow)
            let mut fire_found = false;
            println!("\n--- Fire shader pixel samples (right edge) ---");
            for &offset in &[4.0, 10.0, 20.0, 30.0, 40.0] {
                let sx = fire_content_right + offset;
                if let Some(pixel) = sample_screenshot_pixel_logical(&screenshot2, sx, fire_cy) {
                    println!(
                        "  fire right +{:.0}dp: rgba({}, {}, {}, {})",
                        offset, pixel[0], pixel[1], pixel[2], pixel[3]
                    );
                    // Fire pixels: red channel dominant or any visible non-background color
                    let bg_approx = [69u8, 75, 97]; // tab background in sRGB
                    let diff_r = (pixel[0] as i16 - bg_approx[0] as i16).unsigned_abs();
                    let diff_g = (pixel[1] as i16 - bg_approx[1] as i16).unsigned_abs();
                    let diff_b = (pixel[2] as i16 - bg_approx[2] as i16).unsigned_abs();
                    if diff_r > 15 || diff_g > 15 || diff_b > 15 {
                        fire_found = true;
                    }
                }
            }

            // Also sample top edge
            let fire_content_top = fire_cy - 70.0;
            println!("\n--- Fire shader pixel samples (top edge) ---");
            for &offset in &[4.0, 10.0, 20.0, 30.0] {
                let sy = fire_content_top - offset;
                if let Some(pixel) = sample_screenshot_pixel_logical(&screenshot2, fire_cx, sy) {
                    println!(
                        "  fire top -{:.0}dp: rgba({}, {}, {}, {})",
                        offset, pixel[0], pixel[1], pixel[2], pixel[3]
                    );
                    let bg_approx = [69u8, 75, 97];
                    let diff_r = (pixel[0] as i16 - bg_approx[0] as i16).unsigned_abs();
                    let diff_g = (pixel[1] as i16 - bg_approx[1] as i16).unsigned_abs();
                    let diff_b = (pixel[2] as i16 - bg_approx[2] as i16).unsigned_abs();
                    if diff_r > 15 || diff_g > 15 || diff_b > 15 {
                        fire_found = true;
                    }
                }
            }

            if !fire_found {
                println!("\nFATAL: No fire shader pixels detected outside fire box content rect");

                // Debug scan
                println!("\n--- Debug: horizontal scan from fire center to right ---");
                for i in 0..50 {
                    let dx = i as f32 * 5.0;
                    let sx = fire_cx + dx;
                    if let Some(pixel) = sample_screenshot_pixel_logical(&screenshot2, sx, fire_cy)
                    {
                        println!(
                            "  x+{:.0}: rgba({}, {}, {}, {})",
                            dx, pixel[0], pixel[1], pixel[2], pixel[3]
                        );
                    }
                }

                robot.exit().ok();
                std::process::exit(1);
            }
            println!("\nOK: Fire shader flame pixels detected");

            println!("\n=== Robot Shader Rect Test PASSED ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
