//! Robot test for lazy list redraw bug after Set usize::MAX + Jump to Middle
//!
//! BUG: After clicking "Set usize::MAX" then "Jump to Middle", the lazy list
//! doesn't redraw until user scrolls or does other interaction.
//!
//! This test:
//! 1. Navigates to Lazy List tab  
//! 2. Clicks "Set usize::MAX"
//! 3. Clicks "Jump to Middle"
//! 4. Verifies FirstIndex shows middle value WITHOUT scrolling
//!
//! If the bug exists, FirstIndex will still show 0 after clicking the buttons.

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{
    find_button_in_semantics, find_text_by_prefix_in_semantics, find_text_in_semantics,
};
use desktop_app::app;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== Lazy List Redraw Bug Test ===");
    println!("Testing: Set MAX → Jump to Middle should update UI immediately");

    AppLauncher::new()
        .with_title("LazyList Redraw Bug Test")
        .with_size(1200, 800)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            // Helper to click a button
            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Clicking button '{}'", name);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(100));
                    let _ = robot.wait_for_idle();
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            // Helper to read FirstIndex value
            let read_first_index = || -> Option<usize> {
                find_text_by_prefix_in_semantics(&robot, "FirstIndex: ").and_then(
                    |(_, _, _, _, text)| {
                        text.strip_prefix("FirstIndex: ")?
                            .trim()
                            .parse::<usize>()
                            .ok()
                    },
                )
            };

            // Step 1: Navigate to LazyList tab
            println!("\n=== Step 1: Navigate to 'Lazy List' tab ===");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            // Verify we're on the right tab
            if find_text_in_semantics(&robot, "Lazy List Demo").is_none() {
                println!("FATAL: Lazy List tab content not visible");
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ On Lazy List tab");

            // Step 2: Verify initial state
            println!("\n=== Step 2: Record initial FirstIndex ===");
            let initial_index = read_first_index();
            println!("  Initial FirstIndex: {:?}", initial_index);

            if initial_index != Some(0) {
                println!("  ⚠️  Expected FirstIndex to be 0 initially");
            }

            // Step 3: Click "Set usize::MAX"
            println!("\n=== Step 3: Click 'Set usize::MAX' ===");
            if !click_button("Set usize::MAX") {
                println!("FATAL: Could not find 'Set usize::MAX' button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            // Step 4: Click "Jump to Middle"
            println!("\n=== Step 4: Click 'Jump to Middle' ===");
            if !click_button("Jump to Middle") {
                println!("FATAL: Could not find 'Jump to Middle' button");
                robot.exit().ok();
                std::process::exit(1);
            }

            // Wait for potential redraw
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            // Step 5: Verify FirstIndex is now at middle (NOT still 0)
            println!("\n=== Step 5: Verify FirstIndex updated (CRITICAL TEST) ===");
            let after_jump_index = read_first_index();
            println!("  FirstIndex after Jump to Middle: {:?}", after_jump_index);

            // The middle of usize::MAX should be around 9223372036854775807
            // If it's still 0, the bug is present!
            match after_jump_index {
                Some(index) if index > 1000000 => {
                    println!(
                        "  ✓ PASS: FirstIndex updated to {} (middle of usize::MAX)",
                        index
                    );
                    println!("\n=== Bug is FIXED: LazyList redraws after Jump to Middle ===");
                }
                Some(index) if index == 0 => {
                    println!("");
                    println!("╔════════════════════════════════════════════════════════════════╗");
                    println!("║  FAIL: FirstIndex is still 0 after Jump to Middle!            ║");
                    println!("║                                                                ║");
                    println!("║  This proves the REDRAW BUG exists:                            ║");
                    println!("║  scroll_to_item updated data but UI didn't redraw.            ║");
                    println!("╚════════════════════════════════════════════════════════════════╝");
                    println!("");
                    robot.exit().ok();
                    std::process::exit(1);
                }
                Some(index) => {
                    println!(
                        "  ⚠️  Unexpected index value: {} (expected middle of usize::MAX)",
                        index
                    );
                    println!("  This may indicate partial fix or different issue");
                }
                None => {
                    println!("  ✗ Could not read FirstIndex - UI might be broken");
                    robot.exit().ok();
                    std::process::exit(1);
                }
            }

            // Extra verification: check that middle items are actually visible
            println!("\n=== Step 6: Verify lazy list items actually updated ===");

            // Item #0 should NOT be visible after jumping to middle of usize::MAX
            let found_item_0 = find_text_in_semantics(&robot, "ItemRow #0").is_some()
                || find_text_in_semantics(&robot, "Hello #0").is_some();

            if found_item_0 {
                println!("");
                println!("╔════════════════════════════════════════════════════════════════╗");
                println!("║  FAIL: Item #0 is STILL VISIBLE after Jump to Middle!          ║");
                println!("║                                                                ║");
                println!("║  This is the REDRAW BUG:                                       ║");
                println!("║  - FirstIndex/text stats UPDATED correctly                     ║");
                println!("║  - But LazyColumn ITEMS did NOT rebuild/redraw                 ║");
                println!("║  - The lazy list content is stale until user scrolls           ║");
                println!("╚════════════════════════════════════════════════════════════════╝");
                println!("");
                robot.exit().ok();
                std::process::exit(1);
            } else {
                println!("  ✓ Item #0 correctly scrolled out of view");
            }

            println!("\n=== Lazy List Redraw Bug Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
