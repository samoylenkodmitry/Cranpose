//! Comprehensive fling edge case test
//!
//! This test exercises edge cases that could have bugs:
//! 1. Rapid consecutive flings (fling -> immediate scroll)
//! 2. Scroll to boundary (should stop, not overshoot)
//! 3. Very slow scroll (should NOT trigger fling)
//! 4. Fling interrupted by click (should cancel)
//! 5. Direction reversal (fling up -> fling down)
//! 6. Very fast fling (velocity capping at 3000px/s)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_fling_edge_cases --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button, find_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Fling Edge Case Tests ===\n");

    const TEST_TIMEOUT_SECS: u64 = 90;

    AppLauncher::new()
        .with_title("Fling Edge Cases")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                eprintln!("✗ Test timed out");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Navigate to Lazy List tab
            if let Some((x, y, w, h)) =
                find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"))
            {
                let _ = robot.mouse_move(x + w / 2.0, y + h / 2.0);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(500));
            } else {
                eprintln!("✗ Could not find Lazy List tab");
                let _ = robot.exit();
                return;
            }

            let center_x = 400.0;
            let center_y = 350.0;

            // =========================================================
            // TEST 1: Very slow scroll - should NOT trigger fling
            // =========================================================
            println!("--- Test 1: Very Slow Scroll (no fling expected) ---");

            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(50));

            // Very slow drag - 30px over 500ms = 60px/sec (below 50px/sec threshold)
            for i in 1..=10 {
                let progress = i as f32 / 10.0;
                let _ = robot.mouse_move(center_x, center_y - (30.0 * progress));
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("  ✓ Slow scroll completed - check logs for velocity < 50\n");

            // =========================================================
            // TEST 2: Fast fling to trigger animation
            // =========================================================
            println!("--- Test 2: Fast Fling ---");

            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Fast swipe - 150px in 50ms = 3000px/sec
            for i in 1..=5 {
                let progress = i as f32 / 5.0;
                let _ = robot.mouse_move(center_x, center_y - (150.0 * progress));
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(100));
            println!("  ✓ Fast fling started\n");

            // =========================================================
            // TEST 3: Interrupt fling with click (POTENTIAL BUG)
            // =========================================================
            println!("--- Test 3: Interrupt Fling With Click ---");

            // Click while fling is still animating
            let _ = robot.mouse_move(center_x, center_y - 100.0);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(30));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(300));
            println!("  ✓ Click during fling - check logs for CANCEL\n");

            // =========================================================
            // TEST 4: Rapid consecutive flings (POTENTIAL BUG)
            // =========================================================
            println!("--- Test 4: Rapid Consecutive Flings ---");

            // First fling
            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));
            for i in 1..=3 {
                let _ = robot.mouse_move(center_x, center_y - (100.0 * i as f32 / 3.0));
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();

            // IMMEDIATELY start second fling (don't wait for first to finish)
            std::thread::sleep(Duration::from_millis(30));
            let _ = robot.mouse_move(center_x, center_y - 50.0);
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));
            for i in 1..=3 {
                let _ = robot.mouse_move(center_x, center_y - 50.0 - (100.0 * i as f32 / 3.0));
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(400));
            println!("  ✓ Consecutive flings - check offset continuity\n");

            // =========================================================
            // TEST 5: Direction reversal (POTENTIAL BUG)
            // =========================================================
            println!("--- Test 5: Direction Reversal Mid-Gesture ---");

            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));

            // Scroll up first
            for i in 1..=3 {
                let _ = robot.mouse_move(center_x, center_y - (50.0 * i as f32 / 3.0));
                std::thread::sleep(Duration::from_millis(10));
            }
            // Then quickly reverse direction
            for i in 1..=5 {
                let _ = robot.mouse_move(center_x, center_y - 50.0 + (100.0 * i as f32 / 5.0));
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(400));
            println!("  ✓ Direction reversal - velocity should be positive\n");

            // =========================================================
            // TEST 6: Scroll at boundary (should stop cleanly)
            // =========================================================
            println!("--- Test 6: Scroll At Top Boundary ---");

            // First scroll up to reach near top
            for _ in 0..3 {
                let _ = robot.mouse_move(center_x, center_y + 200.0);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(10));
                for i in 1..=5 {
                    let _ = robot.mouse_move(center_x, center_y + 200.0 + (150.0 * i as f32 / 5.0));
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));
            }
            println!("  ✓ Boundary scrolls - check consumed=0 at boundary\n");

            // =========================================================
            // TEST 7: Frame drop simulation (gaps in samples)
            // =========================================================
            println!("--- Test 7: Simulated Frame Drops ---");

            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));

            // Fast movement with artificial gaps
            let _ = robot.mouse_move(center_x, center_y - 30.0);
            std::thread::sleep(Duration::from_millis(60)); // Simulated frame drop!
            let _ = robot.mouse_move(center_x, center_y - 60.0);
            std::thread::sleep(Duration::from_millis(60)); // Another drop!
            let _ = robot.mouse_move(center_x, center_y - 100.0);
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_move(center_x, center_y - 130.0);
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(400));
            println!("  ✓ Frame drops - should still detect velocity (was the bug!)\n");

            // =========================================================
            // TEST 8: Zero movement then release
            // =========================================================
            println!("--- Test 8: Touch Then Release Without Move ---");

            let _ = robot.mouse_move(center_x, center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("  ✓ No movement - should NOT trigger fling\n");

            // =========================================================
            println!("\n=== All Edge Case Tests Complete ===");
            println!("Review stderr output for [Fling] logs to verify behavior.");

            std::thread::sleep(Duration::from_secs(1));
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
