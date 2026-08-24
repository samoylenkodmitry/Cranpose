//! Robot test for Advance Frame not working bug
//!
//! BUG: In Modifiers tab > Dynamic Modifiers section,
//! the "Advance Frame" button doesn't advance the frame counter.
//!
//! Steps to reproduce:
//! 1. Go to Modifiers tab
//! 2. Click "Dynamic Modifiers" button
//! 3. Note the current Frame counter value
//! 4. Click "Advance Frame" button
//! 5. Frame counter should increase (BUG: it doesn't)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_advance_frame_bug --features robot-app
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot Advance Frame Bug Test ===");
    println!("Testing: 'Advance Frame' button should increment frame counter\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot Advance Frame Bug Test")
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

            // Helper to find frame counter value
            let get_frame_value = |robot: &cranpose::Robot| -> Option<i32> {
                if let Ok(semantics) = robot.get_semantics() {
                    fn find_frame(elem: &cranpose::SemanticElement) -> Option<i32> {
                        if let Some(ref text) = elem.text {
                            // Look for "Frame: N" or just a number that represents frame
                            if text.contains("Frame:") || text.contains("frame:") {
                                // Extract number from "Frame: N" pattern
                                if let Some(num_str) = text.split(':').nth(1) {
                                    if let Ok(n) = num_str.trim().parse::<i32>() {
                                        return Some(n);
                                    }
                                }
                            }
                        }
                        for child in &elem.children {
                            if let Some(v) = find_frame(child) {
                                return Some(v);
                            }
                        }
                        None
                    }
                    for elem in &semantics {
                        if let Some(v) = find_frame(elem) {
                            return Some(v);
                        }
                    }
                }
                None
            };

            // =========================================================
            // STEP 1: Navigate to Modifiers tab
            // =========================================================
            println!("--- Step 1: Navigate to Modifiers tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Modifiers Showcase") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Modifiers Showcase' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Modifiers Showcase tab\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Modifiers Showcase' tab\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 2: Click "Dynamic Modifiers" button
            // =========================================================
            println!("--- Step 2: Click 'Dynamic Modifiers' button ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Dynamic Modifiers"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!(
                    "  Found 'Dynamic Modifiers' button at ({:.1}, {:.1})",
                    cx, cy
                );

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Dynamic Modifiers\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Dynamic Modifiers' button\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 3: Get current frame value
            // =========================================================
            println!("--- Step 3: Get current frame value ---");

            let frame_before = get_frame_value(&robot);
            if let Some(frame) = frame_before {
                println!("  Frame value before: {}\n", frame);
            } else {
                println!("  Could not find Frame value in semantics");
                // Try to find any text containing 'frame' or number patterns
                if let Ok(semantics) = robot.get_semantics() {
                    fn dump_texts(elem: &cranpose::SemanticElement, prefix: &str) {
                        if let Some(ref text) = elem.text {
                            println!("  {}Text: '{}'", prefix, text);
                        }
                        for child in &elem.children {
                            dump_texts(child, &format!("  {}", prefix));
                        }
                    }
                    println!("  Semantics tree:");
                    for elem in &semantics {
                        dump_texts(elem, "");
                    }
                }
                println!();
            }

            // =========================================================
            // STEP 4: Click "Advance Frame" button
            // =========================================================
            println!("--- Step 4: Click 'Advance Frame' button ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Advance Frame"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Advance Frame' button at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Advance Frame\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Advance Frame' button\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 5: Verify frame advanced
            // =========================================================
            println!("--- Step 5: Verify frame advanced ---");

            let frame_after = get_frame_value(&robot);

            match (frame_before, frame_after) {
                (Some(before), Some(after)) => {
                    if after > before {
                        println!("  ✓ PASS: Frame advanced from {} to {}\n", before, after);
                    } else {
                        println!("  ✗ FAIL: Frame did NOT advance!");
                        println!("    Before: {}, After: {}", before, after);
                        println!("    BUG CONFIRMED: Advance Frame button doesn't work.\n");
                        all_passed = false;
                    }
                }
                (None, Some(after)) => {
                    println!("  Frame after: {}", after);
                    println!("  Could not compare (no 'before' value)\n");
                }
                (Some(before), None) => {
                    println!("  Frame before: {}", before);
                    println!("  ✗ FAIL: Frame value disappeared after click!\n");
                    all_passed = false;
                }
                (None, None) => {
                    println!("  Could not find Frame value\n");
                    println!("  Note: Looking for any visible frame-related text...");
                    // Fallback: just check if 'Advance Frame' section is visible
                    if find_in_semantics(&robot, |elem| find_text(elem, "Dynamic")).is_some() {
                        println!("  Dynamic Modifiers section is visible\n");
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
