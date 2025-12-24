//! Robot test to validate visible stats display in LazyList
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_stats --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_text_by_prefix_in_semantics};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Lazy Stats Validation Test ===");

    AppLauncher::new()
        .with_title("LazyStats Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Step 1: Navigate to LazyList tab
            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", x, y);
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(500));
            } else {
                println!("FATAL: 'Lazy List' tab not found");
                robot.exit().ok();
                std::process::exit(1);
            }
            let _ = robot.wait_for_idle();

            // Step 2: Find and print ALL text nodes
            println!("\n--- Step 2: Dump all text nodes ---");
            if let Ok(elements) = robot.get_semantics() {
                robot_test_utils::print_semantics_with_bounds(&elements, 0);
            }

            // Step 3: Look for "Visible:" text
            // Stats are now reactive - they should show non-zero without any interaction
            println!("\n--- Step 3: Check 'Visible:' stats ---");

            let visible_text = find_text_by_prefix_in_semantics(&robot, "Visible:");
            if let Some((x, y, _w, _h, text)) = visible_text {
                println!("  Found: '{}' at ({:.1}, {:.1})", text, x, y);

                // Extract the number
                if let Some(num_str) = text.strip_prefix("Visible:").map(|s| s.trim()) {
                    if let Ok(num) = num_str.parse::<usize>() {
                        if num > 0 {
                            println!("  ✓ PASS: Visible count is {} (non-zero)", num);
                        } else {
                            println!("  ✗ FAIL: Visible count is 0 - reactive stats not working!");
                            robot.exit().ok();
                            std::process::exit(1);
                        }
                    } else {
                        println!("  ⚠️ Could not parse number from '{}'", num_str);
                    }
                }
            } else {
                println!("  ✗ 'Visible:' text not found!");
                robot.exit().ok();
                std::process::exit(1);
            }

            // Step 4: Also check Cached:
            println!("\n--- Step 5: Check 'Cached:' stats ---");
            if let Some((_, _, _, _, text)) = find_text_by_prefix_in_semantics(&robot, "Cached:") {
                println!("  Found: '{}'", text);
            } else {
                println!("  'Cached:' text not found");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
