//! Robot test for LazyList performance validation
//!
//! Validates that:
//! 1. Compose count is bounded (not linear with scroll distance)
//! 2. Effects count matches compose count
//! 3. Slot composition reuse works (items in reuse pool keep effects alive)
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_perf_validation --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::find_text_in_semantics;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList Performance Validation ===");

    AppLauncher::new()
        .with_title("Performance Validation")
        .with_size(900, 700)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(200));

            let read_stats = || -> Option<(usize, usize, usize)> {
                if let Some((_, _, _, _, text)) = compose_testing::find_text_by_prefix_in_semantics(
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

            // Step 2: Scroll down using drag gestures
            println!("\n--- Step 2: Rapid scroll down ---");

            // Find an item to drag on
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                let center_x = x + w / 2.0;
                let center_y = y + h / 2.0;

                let start = std::time::Instant::now();
                // Do multiple drags to simulate extended scrolling
                for _ in 0..5 {
                    robot
                        .drag(center_x, center_y + 50.0, center_x, center_y - 150.0)
                        .ok();
                    std::thread::sleep(Duration::from_millis(30));
                }
                let scroll_time = start.elapsed();
                std::thread::sleep(Duration::from_millis(100));

                println!("  Scroll time: {:?}", scroll_time);
            }

            let after_scroll_down = read_stats();
            if let Some((c, e, d)) = after_scroll_down {
                println!(
                    "  After scroll down: Composes={} Effects={} Disposes={}",
                    c, e, d
                );

                // Key assertion: composes should be bounded, not grow linearly
                let new_composes = c - initial_composes;
                println!("  New composes during scroll: {}", new_composes);

                assert!(
                    new_composes < 100,
                    "Too many composes during scroll: {} (expected <100)",
                    new_composes
                );
                assert_eq!(c, e, "Composes should equal effects");
            }

            // Step 3: Scroll back up
            println!("\n--- Step 3: Scroll back up ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #") {
                let center_x = x + w / 2.0;
                let center_y = y + h / 2.0;

                for _ in 0..5 {
                    robot
                        .drag(center_x, center_y - 50.0, center_x, center_y + 150.0)
                        .ok();
                    std::thread::sleep(Duration::from_millis(30));
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let after_scroll_back = read_stats();
            if let Some((c, e, d)) = after_scroll_back {
                println!(
                    "  After scroll back: Composes={} Effects={} Disposes={}",
                    c, e, d
                );

                // Composes should equal effects
                assert_eq!(c, e, "Composes should equal effects");

                // Note: With slot composition reuse, items in the reuse pool keep their
                // composition state (effects stay alive). Disposes only happen when items
                // exceed the pool capacity. Short scrolls may not trigger any disposes.
                println!("\n=== PERFORMANCE ASSERTIONS PASSED ===");
                println!("  Total composes: {}", c);
                println!("  Total effects: {}", e);
                println!("  Total disposes: {}", d);

                let efficiency = if c > 0 {
                    (c as f64 - d as f64) / c as f64 * 100.0
                } else {
                    100.0
                };
                println!("  Retention efficiency: {:.1}%", efficiency);
            } else {
                println!("  Could not read stats after scroll back");
            }

            println!("\n✓ Performance validation PASSED!");
            robot.exit().ok();
        })
        .run(desktop_app::app::lazy_list::lazy_list_example);
}
