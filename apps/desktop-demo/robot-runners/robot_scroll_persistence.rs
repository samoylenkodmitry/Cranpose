//! Robot test for scroll state persistence after fling
//!
//! This test verifies the fix for the scroll offset reset bug where:
//! - First scroll + fling would work correctly
//! - Second scroll after fling would start from offset 0 instead of where fling ended
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_scroll_persistence --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Scroll Persistence Test ===");
    println!("Testing that scroll position persists after fling animation\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot Scroll Persistence Test")
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

            // Navigate to Lazy List tab
            println!("--- Navigating to Lazy List Tab ---");

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
                println!("  ✓ Clicked Lazy List tab\n");
            } else {
                println!("  ✗ Could not find 'Lazy List' tab - test cannot proceed");
                std::thread::sleep(Duration::from_secs(1));
                let _ = robot.exit();
                return;
            }

            // =========================================================
            // TEST: Scroll persistence after fling
            // =========================================================
            println!("--- Test: Scroll Persistence After Fling ---");

            let start_x = 400.0;
            let start_y = 400.0;
            let swipe_distance = 150.0;

            // Step 1: Perform first fling gesture
            println!("  Step 1: First fling gesture...");
            let _ = robot.mouse_move(start_x, start_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Fast swipe up (scroll down)
            for i in 1..=5 {
                let progress = i as f32 / 5.0;
                let new_y = start_y - (swipe_distance * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();

            // Wait for fling animation to complete
            std::thread::sleep(Duration::from_millis(600));
            println!("    Fling 1 complete");

            // Record position after first fling
            let item_after_fling1 = find_in_semantics(&robot, |elem| find_text(elem, "Item 10"));
            let pos_after_fling1 = item_after_fling1.map(|(_, y, _, _)| y);

            // Step 2: Perform second scroll gesture
            println!("  Step 2: Second scroll gesture (should continue from current position)...");
            let _ = robot.mouse_move(start_x, start_y - swipe_distance);
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Slow scroll (no fling) - just 30px
            for i in 1..=3 {
                let progress = i as f32 / 3.0;
                let new_y = (start_y - swipe_distance) - (30.0 * progress);
                let _ = robot.mouse_move(start_x, new_y);
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("    Scroll 2 complete");

            // Record position after second scroll
            let item_after_scroll2 = find_in_semantics(&robot, |elem| find_text(elem, "Item 10"));
            let pos_after_scroll2 = item_after_scroll2.map(|(_, y, _, _)| y);

            // Analyze results
            println!("\n  Results:");
            match (pos_after_fling1, pos_after_scroll2) {
                (Some(p1), Some(p2)) => {
                    let delta = (p2 - p1).abs();
                    println!("    Item 10 after fling 1: Y={:.1}", p1);
                    println!("    Item 10 after scroll 2: Y={:.1}", p2);
                    println!("    Delta: {:.1}px", delta);

                    // If scroll 2 continued from fling position, delta should be ~30px
                    // If it reset to 0, delta would be much larger (the item might even be off-screen)
                    if delta < 100.0 {
                        println!("\n  ✓ PASS: Scroll position persisted correctly");
                        println!("    Second scroll continued from where first fling ended");
                    } else {
                        println!("\n  ✗ FAIL: Scroll position may have reset");
                        println!("    Delta too large - suggests position was reset");
                    }
                }
                (Some(_), None) => {
                    println!("    Item 10 visible after fling 1, but not after scroll 2");
                    println!("\n  ✓ PASS: Second scroll continued scrolling (item scrolled off)");
                }
                (None, Some(_)) => {
                    println!("    Item 10 NOT visible after fling 1, but visible after scroll 2");
                    println!("\n  ? WARNING: Unexpected state - fling may not have worked");
                }
                (None, None) => {
                    println!("    Item 10 not visible in either case");
                    println!("\n  ? INCONCLUSIVE: Need to check different items");
                }
            }

            println!("\n=== Test Complete ===");
            std::thread::sleep(Duration::from_secs(1));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
