//! Robot test for text input value disappearing bug
//!
//! BUG: When clicking on the "Type here..." text input field,
//! the "Current value: ..." label disappears.
//!
//! Steps to reproduce:
//! 1. Go to Text Input tab
//! 2. Verify "Current value: ..." is visible
//! 3. Click on "Type here..." input field
//! 4. Verify "Current value: ..." is STILL visible (BUG: it disappears)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_text_input_value_disappear --features robot-app
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

mod text_input_robot_helpers;

fn main() {
    env_logger::init();
    println!("=== Robot Text Input Value Disappear Bug Test ===");
    println!("Testing: 'Current value:' should remain visible after clicking input\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot Text Input Value Disappear Bug Test")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("✗ Test timed out after {} seconds", TEST_TIMEOUT_SECS);
                std::process::exit(1);
            });

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            match robot.wait_for_idle() {
                Ok(_) => println!("✓ App ready\n"),
                Err(e) => println!("Note: {}\n", e),
            }

            let mut all_passed = true;

            // =========================================================
            // STEP 1: Navigate to Text Input tab
            // =========================================================
            println!("--- Step 1: Navigate to Text Input tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Text Input") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Text Input' tab at ({:.1}, {:.1})", cx, cy);

                if text_input_robot_helpers::open_text_input_tab(&robot) {
                    println!("  ✓ Switched to Text Input tab\n");
                } else {
                    println!("  ✗ FAIL: Could not verify Text Input tab\n");
                    all_passed = false;
                }
            } else {
                println!("  ✗ FAIL: Could not find 'Text Input' tab\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 2: Verify "Current value:" is visible BEFORE clicking input
            // =========================================================
            println!("--- Step 2: Verify 'Current value:' is visible ---");

            let current_value_before =
                find_in_semantics(&robot, |elem| find_text(elem, "Current value:"));
            if let Some((x, y, _, _)) = current_value_before {
                println!("  Found 'Current value:' at ({:.1}, {:.1})", x, y);
            }

            if current_value_before.is_some() {
                println!("  ✓ 'Current value:' label visible before click\n");
            } else {
                println!("  ✗ FAIL: 'Current value:' not visible even before click!\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 3: Click on "Type here..." input field
            // =========================================================
            println!("--- Step 3: Click on 'Type here...' input field ---");

            if let Some((x, y, w, h)) =
                text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                    find_in_semantics(robot, |elem| find_text(elem, "Type here..."))
                })
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found input field at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked input field\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Type here...' input field\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 4: Verify "Current value:" is STILL visible AFTER clicking
            // =========================================================
            println!("--- Step 4: Verify 'Current value:' is still visible ---");

            let current_value_after =
                find_in_semantics(&robot, |elem| find_text(elem, "Current value:"));
            if let Some((x, y, _, _)) = current_value_after {
                println!("  Found 'Current value:' at ({:.1}, {:.1})", x, y);
            }

            if current_value_after.is_some() {
                println!("  ✓ PASS: 'Current value:' label still visible after click\n");
            } else {
                println!("  ✗ FAIL: 'Current value:' label DISAPPEARED after clicking input!\n");
                println!("         BUG CONFIRMED: Clicking input field removes value label.\n");
                all_passed = false;
            }

            // =========================================================
            // SUMMARY
            // =========================================================
            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ TESTS FAILED - BUG DETECTED");
            }

            std::thread::sleep(Duration::from_secs(1));
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
