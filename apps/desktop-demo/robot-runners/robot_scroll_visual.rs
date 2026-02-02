//! Robot test verifying scroll actually moves content visually.
//!
//! This test catches regressions where scroll value changes but the UI
//! doesn't update (e.g., when layout caches aren't invalidated properly).
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_scroll_visual --features robot-app
//! ```

mod robot_test_utils;

use cranpose::AppLauncher;
use cranpose_testing::{find_button, find_in_semantics, find_text};
use desktop_app::app;
use robot_test_utils::find_bounds_by_text;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Robot Scroll Visual Test ===");
    println!("Verifying that scroll actually moves content visually\n");

    AppLauncher::new()
        .with_title("Robot Scroll Visual Test")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            // Timeout
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(30));
                println!("✗ Test timed out");
                std::process::exit(1);
            });

            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Navigate to Lazy List tab
            let lazy_tab = find_in_semantics(&robot, |elem| find_button(elem, "Lazy List"));

            if let Some((x, y, w, h)) = lazy_tab {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let _ = robot.mouse_move(cx, cy);
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_down();
                std::thread::sleep(Duration::from_millis(50));
                let _ = robot.mouse_up();
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();
                println!("  Switched to Lazy List tab");
            } else {
                println!("  ✗ Could not find Lazy List tab");
                std::process::exit(1);
            }

            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let list_bounds = match find_bounds_by_text(&robot, "LazyListViewport") {
                Some(bounds) => bounds,
                None => {
                    println!("  ✗ Could not find LazyListViewport bounds");
                    std::process::exit(1);
                }
            };
            let center_x = list_bounds.0 + list_bounds.2 * 0.5;
            let start_y = list_bounds.1 + list_bounds.3 * 0.8;
            let end_y = list_bounds.1 + list_bounds.3 * 0.2;
            let drag_distance = (start_y - end_y).abs();

            // Find first item (Hello #0) BEFORE scroll
            let hello_0_before = find_in_semantics(&robot, |elem| find_text(elem, "Hello #0"));
            let y_before = hello_0_before.map(|(_, y, _, _)| y);

            println!("\n--- Test: Scroll should move content visually ---");
            println!("  Item 'Hello #0' Y position before scroll: {:?}", y_before);

            if y_before.is_none() {
                println!("  ✗ Could not find 'Hello #0' - may need different test setup");
                std::process::exit(1);
            }

            // Perform a drag scroll (down to scroll up)
            let _ = robot.mouse_move(center_x, start_y);
            std::thread::sleep(Duration::from_millis(50));
            let _ = robot.mouse_down();
            std::thread::sleep(Duration::from_millis(50));

            // Drag upward (content scrolls down = items move up)
            for i in 0..10 {
                let progress = i as f32 / 10.0;
                let _ = robot.mouse_move(center_x, start_y - (drag_distance * progress));
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = robot.mouse_up();
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            // Find first item AFTER scroll
            let hello_0_after = find_in_semantics(&robot, |elem| find_text(elem, "Hello #0"));
            let y_after = hello_0_after.map(|(_, y, _, _)| y);

            println!("  Item 'Hello #0' Y position after scroll: {:?}", y_after);

            // ===== CRITICAL ASSERTION =====
            // If scroll works, the Y position MUST have changed
            match (y_before, y_after) {
                (Some(before), Some(after)) => {
                    let delta = (after - before).abs();
                    if delta > 10.0 {
                        // Good: item moved significantly
                        println!("  ✓ PASS: Item moved by {:.1}px", delta);
                    } else {
                        // BAD: item didn't move - scroll is visually broken!
                        println!(
                            "  ✗ FAIL: Item only moved {:.1}px - SCROLL IS VISUALLY BROKEN!",
                            delta
                        );
                        println!("         This indicates layout caches aren't being invalidated");
                        println!("         when scroll position changes.");
                        std::process::exit(1);
                    }
                }
                (Some(_), None) => {
                    // Item scrolled off-screen - also good
                    println!("  ✓ PASS: Item scrolled off-screen");
                }
                (None, _) => {
                    println!("  ? INCONCLUSIVE: Could not find item before scroll");
                }
            }

            println!("\n=== Test Summary ===");
            println!("✓ ALL TESTS PASSED - Scroll is working visually");
            let _ = robot.exit();
        })
        .run(|| {
            app::combined_app();
        });
}
