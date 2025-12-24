//! Robot test for LazyList extreme navigation
//!
//! Tests that jumping between start/mid/end with usize::MAX items works correctly.
//! Specifically tests the bug where "set max → mid → end" would fail to reach the end.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_extreme_nav --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::find_text_in_semantics;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList Extreme Navigation Robot Test ===");
    println!("Tests: Set usize::MAX → Jump Mid → Jump End");

    AppLauncher::new()
        .with_title("Extreme Navigation Test")
        .with_size(900, 700)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(200));

            // Find and click "Set usize::MAX" button
            println!("\n--- Step 1: Set usize::MAX items ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Set usize::MAX") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(100));
                println!("  Clicked Set usize::MAX");
            } else {
                println!("  ERROR: Could not find 'Set usize::MAX' button");
                robot.exit().ok();
                return;
            }

            // Check we're still at start (Item #0 visible)
            let at_start = find_text_in_semantics(&robot, "Item #0").is_some();
            println!("  At start (Item #0 visible): {}", at_start);

            // Jump to Middle
            println!("\n--- Step 2: Jump to Middle ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Jump to Middle") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(150));
                println!("  Clicked Jump to Middle");
            }

            // Verify we're not at item 0 anymore
            let still_at_start = find_text_in_semantics(&robot, "Item #0").is_some();
            println!(
                "  Item #0 still visible: {} (should be false)",
                still_at_start
            );

            // Check for item near middle (9223372036854775807)
            // We can't easily check the exact number, but we verify item 0 is gone
            if !still_at_start {
                println!("  ✓ Successfully jumped away from start");
            }

            // Jump to End
            println!("\n--- Step 3: Jump to End ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "⏬ End") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(150));
                println!("  Clicked Jump to End");
            }

            // The key test: after being at middle, jumping to end should work
            // We verify by checking that Item #0 is NOT visible (we're at end, not start)
            let at_start_again = find_text_in_semantics(&robot, "Item #0").is_some();
            println!(
                "  Item #0 visible after End: {} (should be false)",
                at_start_again
            );

            // Use the stats display to help verify - check if we can find item text
            // containing very large numbers (near 18446744073709551614)
            if let Some((_, _, _, _, text)) =
                compose_testing::find_text_by_prefix_in_semantics(&robot, "Item #184")
            {
                println!("  ✓ Found item near end: {}", text);
                println!("\n✓ Jump to End SUCCESS!");
            } else if let Some((_, _, _, _, text)) =
                compose_testing::find_text_by_prefix_in_semantics(&robot, "Item #922")
            {
                // If we're still at middle, this is the bug!
                println!("  ✗ Found item near middle: {}", text);
                println!("\n✗ FAILED - Still at middle after Jump to End!");
            } else {
                println!("  Could not find item text to verify position");
                if !at_start_again {
                    println!("\n✓ Likely at end (not at start)");
                }
            }

            // Jump back to Start to verify it works
            println!("\n--- Step 4: Jump to Start ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "⏫ Start") {
                robot.click(x + w / 2.0, y + h / 2.0).ok();
                std::thread::sleep(Duration::from_millis(150));
                println!("  Clicked Jump to Start");
            }

            let back_at_start = find_text_in_semantics(&robot, "Item #0").is_some();
            if back_at_start {
                println!("  ✓ Jump to Start SUCCESS");
            } else {
                println!("  Jump to Start may have failed");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(desktop_app::app::lazy_list::lazy_list_example);
}
