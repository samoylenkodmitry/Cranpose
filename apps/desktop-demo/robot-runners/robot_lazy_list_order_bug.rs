//! Robot test for LazyList item order corruption bug
//!
//! BUG: When scrolling down in LazyList tab, after item #14 there's
//! a wrong item #0, then #15, then #1, then #16, etc.
//! Items appear interleaved/duplicated incorrectly.
//!
//! Steps to reproduce:
//! 1. Go to Lazy List tab
//! 2. Scroll down past item #14
//! 3. Items should be sequential: #15, #16, #17...
//! 4. BUG: Items appear as #0, #15, #1, #16, #2, #17...
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list_order_bug --features robot-app
//! ```

use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_in_semantics, find_text};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Robot LazyList Item Order Bug Test ===");
    println!("Testing: LazyList items should maintain correct sequential order\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    AppLauncher::new()
        .with_title("Robot LazyList Order Bug Test")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(|robot| {
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

            let mut all_passed = true;

            // Helper to extract visible item numbers from semantics
            let get_visible_items = |robot: &cranpose::Robot| -> Vec<i32> {
                let mut items = Vec::new();
                if let Ok(semantics) = robot.get_semantics() {
                    fn find_items(elem: &cranpose::SemanticElement, items: &mut Vec<(i32, f32)>) {
                        if let Some(ref text) = elem.text {
                            // Match "Hello #N" pattern
                            if text.starts_with("Hello #") {
                                if let Some(num_str) = text.strip_prefix("Hello #") {
                                    if let Ok(n) = num_str.parse::<i32>() {
                                        items.push((n, elem.bounds.y));
                                    }
                                }
                            }
                        }
                        for child in &elem.children {
                            find_items(child, items);
                        }
                    }
                    for elem in &semantics {
                        let mut found = Vec::new();
                        find_items(elem, &mut found);
                        // Sort by Y position to get visual order
                        found.sort_by(|a, b| a.1.total_cmp(&b.1));
                        items.extend(found.iter().map(|(n, _)| *n));
                    }
                }
                items
            };

            // =========================================================
            // STEP 1: Navigate to Lazy List tab
            // =========================================================
            println!("--- Step 1: Navigate to Lazy List tab ---");

            if let Some((x, y, w, h)) = find_button_in_semantics(&robot, "Lazy List") {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!("  Found 'Lazy List' tab at ({:.1}, {:.1})", cx, cy);

                let _ = robot.click(cx, cy);
                std::thread::sleep(Duration::from_millis(500));
                let _ = robot.wait_for_idle();
                println!("  ✓ Clicked Lazy List tab\n");
            } else {
                println!("  ✗ FAIL: Could not find 'Lazy List' tab\n");
                all_passed = false;
            }

            // =========================================================
            // STEP 2: Verify initial items are in order
            // =========================================================
            println!("--- Step 2: Verify initial item order ---");

            let initial_items = get_visible_items(&robot);
            println!("  Initial visible items: {:?}", initial_items);

            if !initial_items.is_empty() {
                // Check if items are sequential
                let mut is_sequential = true;
                for window in initial_items.windows(2) {
                    if window[1] != window[0] + 1 {
                        is_sequential = false;
                        break;
                    }
                }
                if is_sequential {
                    println!("  ✓ Initial items are sequential\n");
                } else {
                    println!("  ✗ FAIL: Initial items are NOT sequential!\n");
                    all_passed = false;
                }
            }

            // =========================================================
            // STEP 3: Find LazyList viewport and scroll down
            // =========================================================
            println!("--- Step 3: Scroll down in LazyList ---");

            // Find the viewport area (look for LazyListViewport or list content area)
            if let Some((x, y, w, h)) = find_in_semantics(&robot, |elem| {
                if elem.role == "Subcompose" && elem.bounds.height > 200.0 {
                    return Some((
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                    ));
                }
                None
            }) {
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                println!(
                    "  Found viewport at ({:.1}, {:.1}) size ({:.1}x{:.1})",
                    x, y, w, h
                );

                // Scroll down multiple times to get past item #14
                for scroll_num in 1..=5 {
                    println!("  Scroll #{}: wheel at ({:.1}, {:.1})", scroll_num, cx, cy);

                    // Move mouse to viewport
                    let _ = robot.mouse_move(cx, cy);
                    std::thread::sleep(Duration::from_millis(50));

                    // Simulate scroll wheel (drag down)
                    let _ = robot.mouse_down();
                    std::thread::sleep(Duration::from_millis(50));

                    // Drag down to scroll
                    let drag_y = cy - 200.0; // Drag up to scroll down
                    let _ = robot.mouse_move(cx, drag_y);
                    std::thread::sleep(Duration::from_millis(100));

                    let _ = robot.mouse_up();
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = robot.wait_for_idle();
                }
                println!("  ✓ Scrolled down\n");
            } else {
                // Fallback: find any list item and scroll from there
                if let Some((x, y, w, _h)) =
                    find_in_semantics(&robot, |elem| find_text(elem, "Hello #"))
                {
                    let cx = x + w / 2.0;
                    let cy = y + 100.0; // Offset to get into list area

                    println!("  Using item position for scroll at ({:.1}, {:.1})", cx, cy);

                    for _ in 0..5 {
                        let _ = robot.mouse_move(cx, cy);
                        std::thread::sleep(Duration::from_millis(50));
                        let _ = robot.mouse_down();
                        std::thread::sleep(Duration::from_millis(50));
                        let _ = robot.mouse_move(cx, cy - 150.0);
                        std::thread::sleep(Duration::from_millis(100));
                        let _ = robot.mouse_up();
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    println!("  ✓ Scrolled down\n");
                } else {
                    println!("  Could not find scroll target\n");
                }
            }

            // =========================================================
            // STEP 4: Check item order after scrolling
            // =========================================================
            println!("--- Step 4: Verify item order after scroll ---");

            let scrolled_items = get_visible_items(&robot);
            println!("  Visible items after scroll: {:?}", scrolled_items);

            if scrolled_items.is_empty() {
                println!("  ✗ FAIL: No items visible after scroll!\n");
                all_passed = false;
            } else {
                // Check for order corruption:
                // 1. Items should be sequential (each item = previous + 1)
                // 2. No duplicates
                // 3. No items going backwards

                let mut order_issues = Vec::new();
                let mut seen = std::collections::HashSet::new();

                for (i, window) in scrolled_items.windows(2).enumerate() {
                    let prev = window[0];
                    let curr = window[1];

                    // Check for duplicates
                    if seen.contains(&curr) {
                        order_issues.push(format!(
                            "Duplicate item #{} at position {}",
                            curr,
                            i + 1
                        ));
                    }
                    seen.insert(curr);

                    // Check for non-sequential (gap > 1 or backwards)
                    if curr != prev + 1 {
                        order_issues.push(format!(
                            "Non-sequential: #{} followed by #{} (expected #{})",
                            prev,
                            curr,
                            prev + 1
                        ));
                    }
                }

                if order_issues.is_empty() {
                    println!("  ✓ PASS: Items are in correct sequential order\n");
                } else {
                    println!("  ✗ FAIL: Item order is CORRUPTED!");
                    for issue in &order_issues {
                        println!("    - {}", issue);
                    }
                    println!("    BUG CONFIRMED: LazyList shows items in wrong order.\n");
                    all_passed = false;
                }
            }

            // =========================================================
            // SUMMARY
            // =========================================================
            println!("\n=== Test Summary ===");
            if all_passed {
                println!("✓ ALL TESTS PASSED");
            } else {
                println!("✗ TESTS FAILED - BUG DETECTED");
            }

            std::thread::sleep(Duration::from_secs(1));
            robot.exit().expect("Failed to exit");
        })
        .run(|| {
            app::combined_app();
        });
}
