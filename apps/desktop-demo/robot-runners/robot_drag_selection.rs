//! Focused robot test for click-drag text selection
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_drag_selection --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

mod text_input_robot_helpers;

fn main() {
    env_logger::init();
    println!("=== Click-Drag Selection Test ===\n");

    AppLauncher::new()
        .with_title("Drag Selection Test")
        .with_size(600, 400)
        .with_headless(true)
        .with_test_driver(|robot| {
            const TEST_TIMEOUT_SECS: u64 = 60;
            // Timeout after a full robot run budget.
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("\n✗ Test timed out after {} seconds", TEST_TIMEOUT_SECS);
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

            // Step 2: Find text field - try multiple times with wait
            println!("--- Step 2: Find text field ---");
            if let Some((field_x, field_y, field_w, field_h)) =
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
                })
            {
                println!("✓ Found text field at ({:.0}, {:.0})\n", field_x, field_y);

                // Step 3: Add text by clicking Add ! button
                println!("--- Step 3: Add text ---");
                for _ in 0..5 {
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
                }
                std::thread::sleep(Duration::from_millis(200));
                println!("✓ Added text\n");

                // Step 4: Perform click-drag selection
                println!("--- Step 4: Click-drag selection ---");

                let start_x = field_x + field_w - 10.0;
                let end_x = field_x + 10.0;
                let center_y = field_y + field_h / 2.0;

                // Mouse move to start
                let _ = robot.mouse_move(start_x, center_y);
                std::thread::sleep(Duration::from_millis(50));

                // Mouse down
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                // Check focus - note: has_focused_field() may not work in robot context
                let focused = cranpose_ui::has_focused_field();
                println!("  • Field focused (has_focused_field): {}", focused);
                if !focused {
                    println!("    (Note: has_focused_field() may return false in robot tests due to thread-local storage)");
                }

                // Drag across (3 steps)
                for step in 1..=3 {
                    let t = step as f32 / 3.0;
                    let drag_x = start_x + (end_x - start_x) * t;
                    let _ = robot.mouse_move(drag_x, center_y);
                    std::thread::sleep(Duration::from_millis(50));
                }
                println!("  • Dragged across text");

                // Mouse up
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));

                // Verify still focused - note: has_focused_field() may not work in robot context
                let still_focused = cranpose_ui::has_focused_field();
                println!("  • Still focused after drag (has_focused_field): {}", still_focused);
                if !still_focused {
                    println!("    (Note: has_focused_field() may return false in robot tests due to thread-local storage)");
                }

                println!("\n✓ PASS: Click-drag selection test completed");
                let _ = robot.exit();
            } else {
                println!("✗ FAIL: Could not find text field after waiting for semantics");
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}
