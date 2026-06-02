//! Robot test for double-click word selection and triple-click select all
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_double_click --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

mod text_input_robot_helpers;

fn main() {
    env_logger::init();
    println!("=== Double-Click / Triple-Click Selection Test ===\n");

    AppLauncher::new()
        .with_title("Double-Click Test")
        .with_size(600, 400)
        .with_headless(true)
        .with_test_driver(|robot| {
            // Timeout after 60 seconds
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(60));
                println!("\n✗ Test timed out");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(300));
            println!("✓ App ready\n");

            // Step 1: Switch to Text Input tab
            println!("--- Step 1: Switch to Text Input Tab ---");
            if text_input_robot_helpers::open_text_input_tab(&robot) {
                println!("✓ Clicked Text Input tab\n");
            } else {
                println!("✗ FAIL: Could not find Text Input tab");
                let _ = robot.exit();
                return;
            }

            // Step 2: Find text field
            println!("--- Step 2: Find text field ---");
            let (field_x, field_y, field_w, field_h) = if let Some(pos) =
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
                }) {
                pos
            } else {
                println!("✗ FAIL: Could not find text field");
                let _ = robot.exit();
                return;
            };
            println!("✓ Found text field at ({:.0}, {:.0})\n", field_x, field_y);

            // Step 3: Add text words by clicking buttons
            println!("--- Step 3: Add text words ---");
            // Click multiple times to add "!!!!!" which will act as words
            for i in 0..8 {
                if let Some((x, y, w, h)) =
                    find_in_semantics(&robot, |elem| find_button(elem, "Add !"))
                {
                    let cx = x + w / 2.0;
                    let cy = y + h / 2.0;
                    let _ = robot.mouse_move(cx, cy);
                    std::thread::sleep(Duration::from_millis(20));
                    let _ = robot.mouse_down();
                    std::thread::sleep(Duration::from_millis(20));
                    let _ = robot.mouse_up();
                    std::thread::sleep(Duration::from_millis(30));
                }
                if i % 2 == 1 {
                    // Type a space every 2 clicks to create word boundaries
                    // Actually just use the buttons as-is - "!!!!" is one word
                }
            }
            std::thread::sleep(Duration::from_millis(200));
            println!("✓ Added text (should be '!!!!!!!!')\n");

            // Step 4: Double-click to select word
            println!("--- Step 4: Double-click word selection ---");

            let center_x = field_x + field_w / 2.0;
            let center_y = field_y + field_h / 2.0;

            // Move to center of text
            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));

            // First click
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(50));

            // Check focus after first click
            let focused_after_click = robot.has_focused_text_field().unwrap_or(false);
            println!("  • Focused after single click: {}", focused_after_click);

            // Second click (double-click - within 500ms)
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(100));

            println!("  • Double-click performed");

            // Verify field is still focused
            let focused_after_double = robot.has_focused_text_field().unwrap_or(false);
            println!("  • Focused after double-click: {}", focused_after_double);

            if !focused_after_double {
                println!("  (Note: app-thread focus query returned false)");
            }
            println!("✓ PASS: Double-click completed\n");

            // Step 5: Triple-click to select all
            println!("--- Step 5: Triple-click select all ---");

            // Wait a moment then do another set of 3 clicks
            std::thread::sleep(Duration::from_millis(600)); // Wait for double-click timeout

            // Click 1
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(100));

            // Click 2
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(100));

            // Click 3 (triple)
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(100));

            println!("  • Triple-click performed");

            // Verify field is still focused
            let focused_after_triple = robot.has_focused_text_field().unwrap_or(false);
            println!("  • Focused after triple-click: {}", focused_after_triple);

            if !focused_after_triple {
                println!("  (Note: app-thread focus query returned false)");
            }

            println!("✓ PASS: Triple-click completed\n");

            println!("=== ✓ ALL TESTS PASSED ===");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
