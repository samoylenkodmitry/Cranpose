//! Robot test for fling (momentum scroll) detection
//!
//! This test validates that:
//! 1. Quick swipe gestures result in velocity detection
//! 2. Velocity is logged when gesture ends (debug mode)
//!
//! Note: This test verifies velocity DETECTION. Full fling animation
//! (scroll continuing after release) requires runtime integration.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_fling --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text};
use compose_ui::{last_fling_velocity, reset_last_fling_velocity};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Fling Test ===");
    println!("Testing velocity detection for fling gestures\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot Fling Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            // Timeout safety
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
            // Navigate to Lazy List tab for scrollable content
            // =========================================================
            println!("--- Navigating to Lazy List Tab ---");

            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"))
            {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));

                println!("  ✓ Clicked Lazy List tab\n");
            } else {
                println!("  ✗ Could not find 'Lazy List' tab\n");
                all_passed = false;
            }

            // =========================================================
            // TEST 1: Verify list content is visible
            // =========================================================
            println!("--- Test 1: Verify List Content ---");

            // Look for an item in the list (should have items like "Item 0", "Item 1", etc.)
            let has_list_item = find_in_semantics(&robot, |elem| find_text(elem, "Item 0"))
                .is_some()
                || find_in_semantics(&robot, |elem| find_text(elem, "Item 1")).is_some()
                || find_in_semantics(&robot, |elem| find_text(elem, "0")).is_some();

            if has_list_item {
                println!("  ✓ PASS: List content visible\n");
            } else {
                println!("  ? List content not found - looking for scrollable area\n");
            }

            // =========================================================
            // TEST 2: Perform quick swipe (fling gesture)
            // =========================================================
            println!("--- Test 2: Quick Swipe (Fling Gesture) ---");
            println!("Performing fast downward swipe to trigger velocity detection...\n");

            // Start position in the middle of the window
            let start_x = 400.0;
            let start_y = 400.0;
            let swipe_distance = 200.0;
            let swipe_steps = 5;
            let step_delay_ms = 10; // Fast swipe - 10ms between steps = ~900 px/sec

            // Reset velocity tracker before swipe
            reset_last_fling_velocity();

            // Record scroll position before swipe (via an item we can track)
            let item_before = find_in_semantics(&robot, |elem| find_text(elem, "Item 5"));
            let before_y = item_before.map(|(_, y, _, _)| y);

            if let Some(y) = before_y {
                println!("  Item 5 before swipe at Y={:.1}", y);
            }

            // Perform the swipe
            let _ = robot.mouse_move(start_x, start_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Quick swipe upward (dragging content up = scrolling down)
            for i in 1..=swipe_steps {
                let progress = i as f32 / swipe_steps as f32;
                let new_y = start_y - (swipe_distance * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(step_delay_ms));
            }

            // Release - this should trigger velocity calculation
            println!("  Releasing after {:.0}px swipe...", swipe_distance);
            let _ = robot.mouse_up();

            // Wait for potential fling animation (even partial)
            std::thread::sleep(Duration::from_millis(300));

            // Check if scroll position changed
            let item_after = find_in_semantics(&robot, |elem| find_text(elem, "Item 5"));
            let after_y = item_after.map(|(_, y, _, _)| y);

            match (before_y, after_y) {
                (Some(by), Some(ay)) => {
                    let delta = ay - by;
                    if delta.abs() > 5.0 {
                        println!(
                            "  ✓ PASS: Item 5 moved by {:.1}px (scroll detected)\n",
                            delta
                        );
                    } else {
                        println!("  ? Item 5 at same position (delta={:.1})\n", delta);
                    }
                }
                (Some(_), None) => {
                    println!("  ✓ PASS: Item 5 no longer visible (scrolled off screen)\n");
                }
                (None, Some(_)) => {
                    println!("  ✓ PASS: Item 5 now visible (scrolled into view)\n");
                }
                (None, None) => {
                    println!("  ? Could not track Item 5 position\n");
                }
            }

            // =========================================================
            // TEST 3: Reverse swipe with velocity assertion
            // =========================================================
            println!("--- Test 3: Reverse Swipe with Velocity Check ---");

            // Reset velocity before the reverse swipe
            reset_last_fling_velocity();

            let _ = robot.mouse_move(start_x, start_y - swipe_distance);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Quick swipe downward
            for i in 1..=swipe_steps {
                let progress = i as f32 / swipe_steps as f32;
                let new_y = (start_y - swipe_distance) + (swipe_distance * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(step_delay_ms));
            }

            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(300));

            // Check velocity was detected
            let velocity = last_fling_velocity();
            println!("  Measured fling velocity: {:.1} px/sec", velocity);

            if velocity.abs() > 50.0 {
                println!("  ✓ PASS: Velocity detected ({:.1} px/sec > 50 threshold)\n", velocity);
            } else {
                println!("  ✗ FAIL: Velocity too low ({:.1} px/sec, expected > 50)\n", velocity);
                all_passed = false;
            }

            // =========================================================
            // Summary
            // =========================================================
            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
                println!("\nNote: This test verifies velocity DETECTION.");
                println!("Full fling animation (momentum scrolling) requires");
                println!("runtime integration which is TODO.");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
            } else {
                println!("✗ SOME TESTS FAILED");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
            }
        })
        .run(|| {
            app::combined_app();
        });
}
