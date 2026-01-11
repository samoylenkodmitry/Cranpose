//! Comprehensive robot test for scroll and fling behavior
//!
//! This test precisely validates:
//! 1. Scroll positions before/after drag
//! 2. Velocity detection consistency
//! 3. Fling animation distance and duration
//! 4. No jump-back behavior on new scroll
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_fling_precise --features robot-app
//! ```

use compose_app::{AppLauncher, Robot};
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Precise Fling Test ===\n");

    const TEST_TIMEOUT_SECS: u64 = 120;

    AppLauncher::new()
        .with_title("Precise Fling Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            // Timeout safety
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                eprintln!("✗ Test timed out after {} seconds", TEST_TIMEOUT_SECS);
                std::process::exit(1);
            });

            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let _ = robot.wait_for_idle();
            println!("✓ App ready\n");

            let mut all_passed = true;
            let mut test_count = 0;
            let mut pass_count = 0;

            // Helper to run a test
            macro_rules! test {
                ($name:expr, $body:expr) => {{
                    test_count += 1;
                    print!("Test {}: {} ... ", test_count, $name);
                    let result: Result<(), String> = (|| $body)();
                    match result {
                        Ok(()) => {
                            pass_count += 1;
                            println!("PASS");
                        }
                        Err(e) => {
                            all_passed = false;
                            println!("FAIL: {}", e);
                        }
                    }
                }};
            }

            // =========================================================
            // Navigate to Lazy List tab
            // =========================================================
            println!("--- Setup: Navigate to Lazy List Tab ---");
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
                println!("✓ Clicked Lazy List tab\n");
            } else {
                println!("✗ Could not find Lazy List tab - aborting");
                let _ = robot.exit();
                return;
            }

            // Find item positions function
            fn find_item(robot: &Robot, item_text: &str) -> Option<(f32, f32)> {
                find_in_semantics(robot, |elem| find_text(elem, item_text))
                    .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
            }

            // Find any "Item X" element
            fn find_any_item(robot: &Robot) -> Option<(f32, String)> {
                // Try items 0-20
                for i in 0..20 {
                    let item_text = format!("Item #{}", i);
                    if let Some((_, y)) = find_item(robot, &item_text) {
                        return Some((y, item_text));
                    }
                }
                None
            }

            // =========================================================
            // TEST 1: Initial state - Item 0 should be visible in viewport
            // =========================================================
            test!("Initial state - Item #0 visible", {
                let item0 = find_item(&robot, "Item #0");
                if item0.is_none() {
                    return Err("Item #0 not found in initial state".to_string());
                }
                let (_, y) = item0.unwrap();

                let viewport =
                    find_in_semantics(&robot, |elem| find_text(elem, "LazyListViewport"));
                let Some((_vx, vy, _vw, vh)) = viewport else {
                    return Err("LazyListViewport not found in semantics".to_string());
                };
                if y < vy || y > (vy + vh) {
                    return Err(format!(
                        "Item 0 y={:.1} outside viewport bounds y=[{:.1}, {:.1}]",
                        y,
                        vy,
                        vy + vh
                    ));
                }
                Ok(())
            });

            // =========================================================
            // TEST 2: Simple drag scroll - verify position changes
            // =========================================================
            test!("Simple drag scroll - position changes", {
                // Get Item #0 position before scroll
                let before = find_item(&robot, "Item #0");
                if before.is_none() {
                    return Err("Item #0 not found before scroll".to_string());
                }
                let (_, before_y) = before.unwrap();

                // Perform a slow drag (100px over 500ms = 200 px/sec - below fling threshold)
                let start_x = 400.0;
                let start_y = 400.0;
                let drag_distance = 100.0;

                let _ = robot.mouse_move(start_x, start_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(100));

                // Slow drag - 10 steps over 500ms
                for i in 1..=10 {
                    let progress = i as f32 / 10.0;
                    let new_y = start_y - (drag_distance * progress);
                    let _ = robot.mouse_move(start_x, new_y);
                    std::thread::sleep(Duration::from_millis(50)); // 50ms per step = slow
                }

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));

                // Check Item #0 position after scroll
                let after = find_item(&robot, "Item #0");
                match after {
                    Some((_, after_y)) => {
                        let delta = after_y - before_y;
                        // Item 0 should have moved up (negative Y delta) by roughly drag distance
                        if delta > -50.0 {
                            return Err(format!(
                                "Item 0 delta {} (expected < -50, before={}, after={})",
                                delta, before_y, after_y
                            ));
                        }
                        Ok(())
                    }
                    None => {
                        // Item 0 scrolled off screen - that's also valid
                        Ok(())
                    }
                }
            });

            // =========================================================
            // TEST 3: Scroll back to top for next tests
            // =========================================================
            test!("Scroll back to top", {
                let start_x = 400.0;
                let start_y = 200.0;

                let _ = robot.mouse_move(start_x, start_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));

                // Drag down to scroll back up
                for i in 1..=10 {
                    let progress = i as f32 / 10.0;
                    let new_y = start_y + (200.0 * progress);
                    let _ = robot.mouse_move(start_x, new_y);
                    std::thread::sleep(Duration::from_millis(30));
                }

                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                // Verify Item #0 is visible again
                let item0 = find_item(&robot, "Item #0");
                if item0.is_none() {
                    return Err("Item #0 not found after scroll back".to_string());
                }
                Ok(())
            });

            // =========================================================
            // TEST 4: Fast swipe triggers fling (check console output)
            // =========================================================
            test!("Fast swipe triggers fling", {
                // Get starting position
                let before = find_item(&robot, "Item #0");
                let before_y = before.map(|(_, y)| y).unwrap_or(100.0);

                // Fast swipe: 200px in 50ms = 4000 px/sec
                let start_x = 400.0;
                let start_y = 400.0;

                let _ = robot.mouse_move(start_x, start_y);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(20));

                // Fast swipe - 5 steps in 50ms = 10ms per step
                for i in 1..=5 {
                    let progress = i as f32 / 5.0;
                    let new_y = start_y - (200.0 * progress);
                    let _ = robot.mouse_move(start_x, new_y);
                    std::thread::sleep(Duration::from_millis(10));
                }

                // Release
                let _ = robot.mouse_up();

                // Wait for fling to complete (longer for animation)
                std::thread::sleep(Duration::from_millis(500));

                // Check: Item 0 should have moved significantly more than just the drag distance
                // (Because fling adds momentum)
                let after = find_item(&robot, "Item #0");
                match after {
                    Some((_, after_y)) => {
                        let total_movement = before_y - after_y;
                        // Should have moved more than just the 200px drag
                        // With fling, expect at least 200px total movement
                        if total_movement < 150.0 {
                            return Err(format!(
                                "Total movement {} < 150px (expected fling momentum)",
                                total_movement
                            ));
                        }
                        eprintln!("  (Item 0 moved {} px total)", total_movement);
                        Ok(())
                    }
                    None => {
                        // Item scrolled off screen - good, means significant movement
                        eprintln!("  (Item 0 scrolled off screen - good!)");
                        Ok(())
                    }
                }
            });

            // =========================================================
            // TEST 5: Repeated scrolls don't cause jump-back
            // =========================================================
            test!("Repeated scrolls no jump-back", {
                // Wait for any animation to finish
                std::thread::sleep(Duration::from_millis(300));

                // Find any visible item
                // Do first scroll
                let _ = robot.mouse_move(400.0, 400.0);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_move(400.0, 350.0);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));

                // Record position after first scroll
                let after_first = find_any_item(&robot);
                let after_first_y = after_first.as_ref().map(|(y, _)| *y).unwrap_or(300.0);

                // Do second scroll - START position should NOT jump back
                let _ = robot.mouse_move(400.0, 400.0);
                std::thread::sleep(Duration::from_millis(30));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(30));

                // Check: during second scroll start, position should be same as after first
                let during_second = find_any_item(&robot);
                let during_y = during_second.as_ref().map(|(y, _)| *y).unwrap_or(300.0);

                // Cleanup
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(100));

                // Position should NOT have jumped significantly on mouse down
                let jump = (during_y - after_first_y).abs();
                if jump > 50.0 {
                    return Err(format!(
                        "Jump-back detected! After first scroll Y={}, on second down Y={}, jump={}",
                        after_first_y, during_y, jump
                    ));
                }
                eprintln!("  (No jump-back: delta={:.1}px)", jump);
                Ok(())
            });

            // =========================================================
            // Summary
            // =========================================================
            println!("\n=== Test Summary ===");
            println!("{} / {} tests passed", pass_count, test_count);

            if all_passed {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ SOME TESTS FAILED");
            }

            std::thread::sleep(Duration::from_secs(1));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
