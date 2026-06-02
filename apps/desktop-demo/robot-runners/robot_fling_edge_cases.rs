//! Comprehensive fling edge case test
//!
//! This test exercises edge cases that could have bugs:
//! 1. Rapid consecutive flings (fling -> immediate scroll)
//! 2. Scroll to boundary (should stop, not overshoot)
//! 3. Very slow scroll (should NOT trigger fling)
//! 4. Fling interrupted by click (should cancel)
//! 5. Direction reversal (fling up -> fling down)
//! 6. Very fast fling (velocity capped by MAX_FLING_VELOCITY)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_fling_edge_cases --features robot-app
//! ```

use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_bounds_by_text, visible_bounds_in_viewport};
use cranpose_testing::{find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;
use std::time::Duration;

struct ListViewport {
    center_x: f32,
    center_y: f32,
    upper_y: f32,
    lower_y: f32,
}

impl ListViewport {
    fn clamp_y(&self, y: f32) -> f32 {
        y.clamp(self.upper_y, self.lower_y)
    }
}

fn fetch_list_viewport(robot: &Robot) -> Option<ListViewport> {
    let list_bounds = find_bounds_by_text(robot, "LazyListViewport")?;
    let visible_bounds = visible_bounds_in_viewport(robot, list_bounds, 12.0)?;

    let center_x = visible_bounds.0 + visible_bounds.2 * 0.5;
    let center_y = visible_bounds.1 + visible_bounds.3 * 0.5;
    let upper_y = visible_bounds.1 + visible_bounds.3 * 0.2;
    let lower_y = visible_bounds.1 + visible_bounds.3 * 0.8;

    Some(ListViewport {
        center_x,
        center_y,
        upper_y,
        lower_y,
    })
}

fn main() {
    env_logger::init();
    println!("=== Fling Edge Case Tests ===\n");

    const TEST_TIMEOUT_SECS: u64 = 180;

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
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
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

            let mut viewport = match fetch_list_viewport(&robot) {
                Some(bounds) => bounds,
                None => {
                    eprintln!("✗ LazyListViewport is not visible in the viewport");
                    let _ = robot.exit();
                    return;
                }
            };

            fn find_item_center_y(robot: &Robot, item_text: &str) -> Option<f32> {
                find_in_semantics(robot, |elem| find_text(elem, item_text))
                    .map(|(_x, y, _w, h)| y + h / 2.0)
            }

            fn find_any_item(robot: &Robot) -> Option<(String, f32)> {
                for i in 0..30 {
                    let item_text = format!("Item #{}", i);
                    if let Some(center_y) = find_item_center_y(robot, &item_text) {
                        return Some((item_text, center_y));
                    }
                }
                None
            }

            // =========================================================
            // TEST 1: Very slow scroll - should NOT trigger fling
            // =========================================================
            println!("--- Test 1: Very Slow Scroll (no fling expected) ---");

            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(50));

            // Very slow drag - 30px over 500ms = 60px/sec (below 50px/sec threshold)
            for i in 1..=10 {
                let progress = i as f32 / 10.0;
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - (30.0 * progress)),
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            println!("  ✓ Slow scroll completed - check logs for velocity < 50\n");

            // =========================================================
            // TEST 2: Fast fling to trigger animation
            // =========================================================
            println!("--- Test 2: Fast Fling ---");

            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
            std::thread::sleep(Duration::from_millis(100));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(20));

            // Fast swipe - 150px in 50ms = 3000px/sec (below max cap)
            for i in 1..=5 {
                let progress = i as f32 / 5.0;
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - (150.0 * progress)),
                );
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
            let _ = robot.mouse_move(
                viewport.center_x,
                viewport.clamp_y(viewport.center_y - 100.0),
            );
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
            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));
            for i in 1..=3 {
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - (100.0 * i as f32 / 3.0)),
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();

            // IMMEDIATELY start second fling (don't wait for first to finish)
            std::thread::sleep(Duration::from_millis(30));
            let _ = robot.mouse_move(
                viewport.center_x,
                viewport.clamp_y(viewport.center_y - 50.0),
            );
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));
            for i in 1..=3 {
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - 50.0 - (100.0 * i as f32 / 3.0)),
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(400));
            println!("  ✓ Consecutive flings - check offset continuity\n");

            // =========================================================
            // TEST 5: Direction reversal (POTENTIAL BUG)
            // =========================================================
            println!("--- Test 5: Direction Reversal Mid-Gesture ---");

            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));

            // Scroll up first
            for i in 1..=3 {
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - (50.0 * i as f32 / 3.0)),
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            // Then quickly reverse direction
            for i in 1..=5 {
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y - 50.0 + (100.0 * i as f32 / 5.0)),
                );
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
                let _ = robot.mouse_move(
                    viewport.center_x,
                    viewport.clamp_y(viewport.center_y + 200.0),
                );
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(10));
                for i in 1..=5 {
                    let _ = robot.mouse_move(
                        viewport.center_x,
                        viewport.clamp_y(viewport.center_y + 200.0 + (150.0 * i as f32 / 5.0)),
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(300));
            }
            println!("  ✓ Boundary scrolls - check consumed=0 at boundary\n");

            // =========================================================
            // TEST 7: Frame drop simulation (gaps in samples)
            // =========================================================
            // Move away from the top boundary so the fling has room to continue.
            let _ = robot.mouse_move(viewport.center_x, viewport.lower_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_move(viewport.center_x, viewport.upper_y);
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();
            if let Some(updated) = fetch_list_viewport(&robot) {
                viewport = updated;
            }

            println!("--- Test 7: Simulated Frame Drops ---");

            let (tracked_label, _before_y) = match find_any_item(&robot) {
                Some(value) => value,
                None => {
                    eprintln!("✗ Could not find a visible item before frame drop test");
                    std::process::exit(1);
                }
            };

            if let Err(err) = robot.reset_last_fling_velocity() {
                eprintln!("✗ Failed to reset fling velocity: {err}");
                std::process::exit(1);
            }

            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(10));

            // Fast movement with artificial gaps
            let drag_base_y = viewport.center_y;
            let _ = robot.mouse_move(viewport.center_x, drag_base_y - 30.0);
            std::thread::sleep(Duration::from_millis(60)); // Simulated frame drop!
            let _ = robot.mouse_move(viewport.center_x, drag_base_y - 60.0);
            std::thread::sleep(Duration::from_millis(60)); // Another drop!
            let _ = robot.mouse_move(viewport.center_x, drag_base_y - 100.0);
            std::thread::sleep(Duration::from_millis(10));
            let _ = robot.mouse_move(viewport.center_x, drag_base_y - 130.0);
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(50));

            let post_release_y = find_item_center_y(&robot, &tracked_label);
            std::thread::sleep(Duration::from_millis(300));
            let after_fling_y = find_item_center_y(&robot, &tracked_label);
            let measured_velocity = match robot.last_fling_velocity() {
                Ok(value) => value.abs(),
                Err(err) => {
                    eprintln!("✗ Failed to query fling velocity: {err}");
                    std::process::exit(1);
                }
            };

            match (post_release_y, after_fling_y) {
                (Some(post_y), Some(after_y)) => {
                    let additional = (post_y - after_y).abs();
                    if additional < 15.0 && measured_velocity < 50.0 {
                        eprintln!(
                            "✗ Frame drop fling too small: {:.1}px (velocity {:.1} px/s)",
                            additional, measured_velocity
                        );
                        std::process::exit(1);
                    } else if additional < 15.0 {
                        println!(
                            "  WARN: Low visual movement ({:.1}px) but fling velocity {:.1} px/s",
                            additional, measured_velocity
                        );
                    }
                }
                _ => {
                    // If the tracked item scrolled off-screen, momentum likely occurred.
                }
            }

            println!("  ✓ Frame drops - fling momentum detected\n");

            // =========================================================
            // TEST 8: Zero movement then release
            // =========================================================
            println!("--- Test 8: Touch Then Release Without Move ---");

            let _ = robot.mouse_move(viewport.center_x, viewport.center_y);
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
