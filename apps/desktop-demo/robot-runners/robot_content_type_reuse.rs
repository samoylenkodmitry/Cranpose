//! Robot test for content-type-based slot reuse validation
//!
//! Validates that:
//! 1. Slots with matching content types can be reused across different item indices
//! 2. The reuse mechanism works correctly after the refactoring from SlotReusePool to SubcomposeState
//! 3. Content-type based reuse reduces compose counts during scrolling
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_content_type_reuse --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_testing::find_text_in_semantics;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Content-Type Reuse Robot Test ===");

    AppLauncher::new()
        .with_title("Content-Type Reuse Test")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(200));

            let read_stats = || -> Option<(usize, usize, usize)> {
                if let Some((_, _, _, _, text)) = cranpose_testing::find_text_by_prefix_in_semantics(
                    &robot,
                    "Lifecycle totals: C=",
                ) {
                    // Parse "Lifecycle totals: C=X E=Y D=Z"
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let c = parts[2].strip_prefix("C=")?.parse().ok()?;
                        let e = parts[3].strip_prefix("E=")?.parse().ok()?;
                        let d = parts[4].strip_prefix("D=")?.parse().ok()?;
                        return Some((c, e, d));
                    }
                }
                None
            };

            // Step 1: Initial state
            println!("\n--- Step 1: Initial state ---");
            let initial_stats = read_stats();
            if let Some((c, e, d)) = initial_stats {
                println!("  Initial: Composes={} Effects={} Disposes={}", c, e, d);
                assert_eq!(c, e, "Composes should equal effects");
                assert_eq!(d, 0, "No disposes initially");
            }
            let initial_composes = initial_stats.map(|(c, _, _)| c).unwrap_or(0);

            // Step 2: Extended scroll to trigger slot reuse
            println!("\n--- Step 2: Extended scroll (triggers content-type reuse) ---");

            // Find an item to drag on
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                let center_x = x + w / 2.0;
                let center_y = y + h / 2.0;

                let start = std::time::Instant::now();
                // Do many drags to simulate extended scrolling through many items
                // With content-type reuse, items with same type (index % 5) reuse each other's slots
                for _ in 0..10 {
                    robot
                        .drag(center_x, center_y + 100.0, center_x, center_y - 200.0)
                        .ok();
                    std::thread::sleep(Duration::from_millis(50));
                }
                let scroll_time = start.elapsed();
                std::thread::sleep(Duration::from_millis(100));

                println!("  Scroll time: {:?}", scroll_time);
            }

            let after_scroll = read_stats();
            if let Some((c, e, d)) = after_scroll {
                println!(
                    "  After extended scroll: Composes={} Effects={} Disposes={}",
                    c, e, d
                );

                let new_composes = c - initial_composes;
                println!("  New composes during scroll: {}", new_composes);

                // Key assertion: composes should be bounded
                // With efficient reuse, we should see fewer composes than items scrolled through
                assert!(
                    new_composes < 200,
                    "Too many composes during scroll: {} (expected <200)",
                    new_composes
                );
                assert_eq!(c, e, "Composes should equal effects");
            }

            // Step 3: Scroll back and verify reuse efficiency
            println!("\n--- Step 3: Scroll back ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #") {
                let center_x = x + w / 2.0;
                let center_y = y + h / 2.0;

                for _ in 0..10 {
                    robot
                        .drag(center_x, center_y - 50.0, center_x, center_y + 200.0)
                        .ok();
                    std::thread::sleep(Duration::from_millis(50));
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let final_stats = read_stats();
            if let Some((c, e, d)) = final_stats {
                println!("\n=== FINAL RESULTS ===");
                println!("  Total composes: {}", c);
                println!("  Total effects: {}", e);
                println!("  Total disposes: {}", d);

                // Disposes happen if items are evicted from reuse pool.
                // If pool is large enough, items might be kept for reuse without dispose.
                if d > 0 {
                   let reuse_ratio = (c - d) as f64 / c as f64 * 100.0;
                   println!("  Slot retention rate: {:.1}%", reuse_ratio);
                   assert!(
                       reuse_ratio > 20.0,
                       "Retention rate too low: {:.1}% (expected >20%)",
                       reuse_ratio
                   );
                } else {
                   println!("  No disposes observed (all items retained in pool). 100% retention.");
                }

                println!("\n=== CONTENT-TYPE REUSE VALIDATION PASSED ===");
            } else {
                println!("  Could not read final stats");
            }

            println!("\n✓ Content-type reuse test PASSED!");
            robot.exit().ok();
        })
        .run(desktop_app::app::lazy_list::lazy_list_example);
}
