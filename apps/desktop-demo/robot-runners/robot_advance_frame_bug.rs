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

            let get_frame_value = |robot: &cranpose::Robot| -> Option<i32> {
                if let Ok(semantics) = robot.get_semantics() {
                    fn find_frame(elem: &cranpose::SemanticElement) -> Option<i32> {
                        if let Some(ref text) = elem.text {
                            if text.contains("Frame:") || text.contains("frame:") {
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

            println!("--- Step 3: Get current frame value ---");

            let frame_before = get_frame_value(&robot);
            if let Some(frame) = frame_before {
                println!("  Frame value before: {}\n", frame);
            } else {
                println!("  Could not find Frame value in semantics");
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
                    if find_in_semantics(&robot, |elem| find_text(elem, "Dynamic")).is_some() {
                        println!("  Dynamic Modifiers section is visible\n");
                    }
                }
            }

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
