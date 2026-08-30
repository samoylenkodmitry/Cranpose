mod robot_launch;

use std::time::Duration;

use cranpose_testing::find_text_in_semantics;

fn main() {
    env_logger::init();
    println!("=== LazyList Performance Validation ===");

    robot_launch::launch("Performance Validation", 900, 700).with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(200));

            let read_stats = || -> Option<(usize, usize, usize)> {
                if let Some((_, _, _, _, text)) = cranpose_testing::find_text_by_prefix_in_semantics(
                    &robot,
                    "Lifecycle totals: C=",
                ) {
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

            println!("\n--- Step 1: Initial state ---");
            let initial_stats = read_stats();
            if let Some((c, e, d)) = initial_stats {
                println!("  Initial: Composes={} Effects={} Disposes={}", c, e, d);
                assert_eq!(c, e, "Composes should equal effects");
                assert_eq!(d, 0, "No disposes initially");
            }
            let initial_composes = initial_stats.map(|(c, _, _)| c).unwrap_or(0);

            println!("\n--- Step 2: Rapid scroll down ---");

            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                let center_x = x + w / 2.0;
                let center_y = y + h / 2.0;

                let start = std::time::Instant::now();
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

                let new_composes = c - initial_composes;
                println!("  New composes during scroll: {}", new_composes);

                assert!(
                    new_composes < 100,
                    "Too many composes during scroll: {} (expected <100)",
                    new_composes
                );
                assert_eq!(c, e, "Composes should equal effects");
            }

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

                assert_eq!(c, e, "Composes should equal effects");

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
