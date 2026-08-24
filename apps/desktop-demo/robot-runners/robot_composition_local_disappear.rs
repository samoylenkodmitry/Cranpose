//! Robot test for CompositionLocal green box disappearing bug
//!
//! BUG: In CompositionLocal Test tab, clicking "Increment" twice
//! causes the green "READING..." box to disappear.
//!
//! Steps to reproduce:
//! 1. Go to CompositionLocal Test tab
//! 2. Verify green "READING..." box is visible
//! 3. Click "Increment" button once
//! 4. Verify green box still visible
//! 5. Click "Increment" button again
//! 6. Green box DISAPPEARS (BUG)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_composition_local_disappear --features robot-app
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot CompositionLocal Disappear Bug Test ===");
    println!("Testing: Green 'READING...' box should remain after clicking Increment\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot CompositionLocal Bug Test")
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

            // Helper to find "READING" text (the green box content)
            // Uses find_text which recursively searches children
            let find_reading_box = |robot: &cranpose::Robot| -> Option<(f32, f32, f32, f32)> {
                find_in_semantics(robot, |elem| find_text(elem, "READING"))
            };

            // =========================================================
            // STEP 1: Navigate to CompositionLocal Test tab
            // =========================================================
            println!("--- Step 1: Navigate to CompositionLocal Test tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "CompositionLocal Test") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!(
                    "  Found 'CompositionLocal Test' tab at ({:.1}, {:.1})",
                    cx, cy
                );

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked CompositionLocal Test tab\n");
            } else {
                println!("  ✗ FAIL: Could not find 'CompositionLocal Test' tab\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 2: Verify green "READING" box is visible initially
            // =========================================================
            println!("--- Step 2: Verify 'READING' box is visible ---");

            let reading_box_initial = find_reading_box(&robot);
            if reading_box_initial.is_some() {
                println!("  ✓ 'READING...' box is visible initially\n");
            } else {
                // Try to find what IS visible
                println!("  Looking for any text containing 'READING' or 'Local'...");
                if let Ok(semantics) = robot.get_semantics() {
                    fn dump(elem: &cranpose::SemanticElement, depth: usize) {
                        if let Some(ref text) = elem.text {
                            if text.contains("Local")
                                || text.contains("READING")
                                || text.contains("Value")
                            {
                                println!("  {} Found: '{}'", "  ".repeat(depth), text);
                            }
                        }
                        for child in &elem.children {
                            dump(child, depth + 1);
                        }
                    }
                    for elem in &semantics {
                        dump(elem, 0);
                    }
                }
                println!("  ✗ FAIL: 'READING...' box not visible initially!\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 3: Click "Increment" button once
            // =========================================================
            println!("--- Step 3: Click 'Increment' button (first time) ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Increment"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Increment (1st time)\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Increment' button\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 4: Verify green box still visible after first click
            // =========================================================
            println!("--- Step 4: Verify 'READING' box after 1st click ---");

            let reading_box_after_1 = find_reading_box(&robot);
            if reading_box_after_1.is_some() {
                println!("  ✓ 'READING...' box still visible after 1st click\n");
            } else {
                println!("  ✗ FAIL: 'READING...' box disappeared after 1st click!\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 5: Click "Increment" button again
            // =========================================================
            println!("--- Step 5: Click 'Increment' button (second time) ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Increment"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Increment' button at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Increment (2nd time)\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Increment' button\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 6: Verify green box STILL visible after second click
            // =========================================================
            println!("--- Step 6: Verify 'READING' box after 2nd click ---");

            let reading_box_after_2 = find_reading_box(&robot);
            if reading_box_after_2.is_some() {
                println!("  ✓ PASS: 'READING...' box still visible after 2nd click\n");
            } else {
                println!("  ✗ FAIL: 'READING...' box DISAPPEARED after 2nd click!");
                println!(
                    "    BUG CONFIRMED: Green box disappears when clicking Increment twice.\n"
                );
                all_passed = false;

                // Dump what IS visible now
                println!("  Current semantics:");
                if let Ok(semantics) = robot.get_semantics() {
                    fn dump(elem: &cranpose::SemanticElement, depth: usize) {
                        let indent = "  ".repeat(depth);
                        if let Some(ref text) = elem.text {
                            println!("  {}role={} text='{}'", indent, elem.role, text);
                        }
                        for child in &elem.children {
                            dump(child, depth + 1);
                        }
                    }
                    for elem in &semantics {
                        dump(elem, 0);
                    }
                }
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
