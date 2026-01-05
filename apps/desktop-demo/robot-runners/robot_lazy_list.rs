//! Robot test for LazyList tab - validates item positions, bounds, and rendering
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{
    find_button_in_semantics, find_in_semantics, find_text_by_prefix_in_semantics, find_text_exact,
    find_text_in_semantics,
};
use desktop_app::app;
use robot_test_utils::{find_element_by_text_exact, print_semantics_with_bounds, union_bounds};
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList Robot Test (with bounds validation) ===");

    AppLauncher::new()
        .with_title("LazyList Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(100));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            let verify_text = |text: &str| -> bool {
                if let Some((x, y, _, _)) = find_text_in_semantics(&robot, text) {
                    println!("  ✓ Found text '{}' at ({:.1}, {:.1})", text, x, y);
                    true
                } else {
                    println!("  ✗ Text '{}' not found!", text);
                    false
                }
            };

            let verify_text_prefix = |prefix: &str| -> bool {
                if let Some((x, y, _, _, text)) = find_text_by_prefix_in_semantics(&robot, prefix) {
                    println!(
                        "  ✓ Found text '{}' (prefix '{}') at ({:.1}, {:.1})",
                        text, prefix, x, y
                    );
                    true
                } else {
                    println!("  ✗ Text with prefix '{}' not found!", prefix);
                    false
                }
            };

            let read_stat = |prefix: &str| -> Option<usize> {
                find_text_by_prefix_in_semantics(&robot, prefix).and_then(|(_, _, _, _, text)| {
                    text.strip_prefix(prefix)?.trim().parse::<usize>().ok()
                })
            };

            let wait_for_text = |text: &str| -> bool {
                for _ in 0..20 {
                    if find_text_in_semantics(&robot, text).is_some() {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                false
            };

            // Find items with FULL BOUNDS (x, y, width, height)
            let find_visible_items_with_bounds = || {
                let mut items: Vec<(usize, (f32, f32, f32, f32), (f32, f32, f32, f32))> =
                    Vec::new(); // (index, row_bounds, group_bounds)
                for i in 0..20 {
                    let item_text = format!("ItemRow #{}", i);
                    let item_bounds = find_in_semantics(&robot, |elem| {
                        find_text_exact(elem, &item_text)
                    });
                    if let Some(row_bounds) = item_bounds {
                        let hello_text = format!("Hello #{}", i);
                        let hello_bounds = find_in_semantics(&robot, |elem| {
                            find_text_exact(elem, &hello_text)
                        });
                        let group_bounds = union_bounds(row_bounds, hello_bounds);
                        items.push((i, row_bounds, group_bounds));
                    }
                }
                items
            };

            // Step 1: Navigate to LazyList tab
            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();
            if !wait_for_text("Lazy List Demo") {
                println!("  ⚠️  Lazy List tab content did not appear within timeout");
            }

            // Step 2: Verify tab content
            println!("\n--- Step 2: Verify LazyList content ---");
            let has_title = verify_text("Lazy List Demo");
            let has_count = verify_text_prefix("Virtualized list with");
            let has_item = verify_text("Hello #0");
            if !(has_title && has_count && has_item) {
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
            }

            let mut has_issues = false;

            // Step 2b: Capture tree rects for LazyList viewport
            println!("\n--- Step 2b: Capture LazyList tree rects ---");
            let semantics = robot.get_semantics().ok();
            let list_bounds = semantics
                .as_deref()
                .and_then(|elements| find_element_by_text_exact(elements, "LazyListViewport"))
                .map(|elem| (elem.bounds.x, elem.bounds.y, elem.bounds.width, elem.bounds.height));
            let list_bounds_missing = list_bounds.is_none();

            if let Some((x, y, w, h)) = list_bounds {
                println!(
                    "  ✓ LazyListViewport bounds=({:.1},{:.1},{:.1},{:.1})",
                    x, y, w, h
                );
                let expected_height = 400.0;
                let allowed_delta = 2.0;
                if (h - expected_height).abs() > allowed_delta {
                    println!(
                        "  ⚠️  LazyListViewport height mismatch: expected ~{:.1}, got {:.1}",
                        expected_height, h
                    );
                    has_issues = true;
                }
                if let Some(elements) = semantics.as_deref() {
                    if let Some(list_elem) = find_element_by_text_exact(elements, "LazyListViewport")
                    {
                        print_semantics_with_bounds(std::slice::from_ref(list_elem), 1);
                    }
                }
            } else {
                println!("  ✗ LazyListViewport semantics not found");
            }

            // Step 3: Find all visible items with FULL BOUNDS
            println!("\n--- Step 3: Validate item BOUNDS (detecting overlaps) ---");
            let mut items = find_visible_items_with_bounds();
            items.sort_by(|a, b| {
                let a_y = (a.2).1;
                let b_y = (b.2).1;
                a_y.partial_cmp(&b_y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            if items.is_empty() {
                println!("  ✗ CRITICAL: No items found! LazyColumn not rendering.");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("  Found {} visible items:", items.len());
            let avg_height = items
                .iter()
                .map(|(_, _, group)| group.3)
                .sum::<f32>()
                / items.len().max(1) as f32;
            let max_reasonable_height = avg_height * 2.5;

            let mut has_bounds_mismatch = list_bounds_missing;
            for (idx, row, group) in &items {
                println!(
                    "    Item #{}: row=({:.1}, {:.1}, {:.1} x {:.1}) group=({:.1}, {:.1}, {:.1} x {:.1})",
                    idx, row.0, row.1, row.2, row.3, group.0, group.1, group.2, group.3
                );

                // Check for suspicious sizes
                if group.3 < 10.0 {
                    println!("      ⚠️  Height too small! Expected ~40-60px");
                    has_issues = true;
                }
                if group.3 > max_reasonable_height {
                    println!("      ⚠️  Height too large!");
                    has_issues = true;
                }

                if let Some((list_x, _list_y, list_w, _list_h)) = list_bounds {
                    let expected = list_w;
                    if (row.2 - expected).abs() > 2.0 {
                        println!(
                            "      ⚠️  Width mismatch! Expected {:.1}, got {:.1}",
                            expected, row.2
                        );
                        has_bounds_mismatch = true;
                    }
                    if group.0 < list_x - 1.0
                        || (group.0 + group.2) > (list_x + list_w + 1.0)
                    {
                        println!("      ⚠️  Item exceeds LazyListViewport bounds");
                        has_bounds_mismatch = true;
                    }
                } else if row.2 < 100.0 {
                    println!("      ⚠️  Width too small! Expected near full width");
                    has_issues = true;
                }
            }

            // Check for overlaps
            println!("\n--- Step 4: Check for OVERLAPPING items ---");
            let mut overlap_count = 0;
            for i in 0..items.len() {
                for j in (i+1)..items.len() {
                    let (idx_a, _row_a, group_a) = items[i];
                    let (idx_b, _row_b, group_b) = items[j];

                    // Check if item j starts before item i ends
                    let item_a_bottom = group_a.1 + group_a.3;
                    if group_b.1 < item_a_bottom {
                        println!("  ⚠️  OVERLAP: Item #{} (bottom={:.1}) overlaps with Item #{} (top={:.1})",
                            idx_a, item_a_bottom, idx_b, group_b.1);
                        overlap_count += 1;
                    }
                }
            }

            if overlap_count > 0 {
                println!("  ✗ Found {} overlapping item pairs!", overlap_count);
                has_issues = true;
            } else {
                println!("  ✓ No overlapping items detected");
            }

            // Check vertical gaps
            println!("\n--- Step 5: Check item SPACING ---");
            for i in 1..items.len() {
                let (idx_prev, _row_prev, group_prev) = items[i - 1];
                let (idx_curr, _row_curr, group_curr) = items[i];
                let gap = group_curr.1 - (group_prev.1 + group_prev.3);
                if gap < -1.0 {
                    println!("  ⚠️  Negative gap ({:.1}px) between Item #{} and #{}", gap, idx_prev, idx_curr);
                } else if gap > 50.0 {
                    println!("  ⚠️  Large gap ({:.1}px) between Item #{} and #{}", gap, idx_prev, idx_curr);
                } else {
                    println!("  Item #{} -> #{}: gap = {:.1}px", idx_prev, idx_curr, gap);
                }
            }

            // Step 6: Validate reuse pool stats after scrolling
            println!("\n--- Step 6: Validate reuse pool stats ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Item #0") {
                robot
                    .drag(
                        x + w / 2.0,
                        y + h / 2.0 + 120.0,
                        x + w / 2.0,
                        y + h / 2.0 - 120.0,
                    )
                    .ok();
                std::thread::sleep(Duration::from_millis(200));
            }

            let mut cached_value = None;
            for _ in 0..5 {
                if let Some(value) = read_stat("Cached: ") {
                    cached_value = Some(value);
                    if value > 0 {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }

            let cached_value = cached_value.unwrap_or(0);
            const MAX_REUSE_PER_TYPE: usize = 7;
            const CONTENT_TYPE_BUCKETS: usize = 5;
            let max_cached = MAX_REUSE_PER_TYPE * CONTENT_TYPE_BUCKETS;

            if cached_value > max_cached {
                println!(
                    "  ✗ Cached count too high: {} (expected <= {})",
                    cached_value, max_cached
                );
                has_issues = true;
            } else {
                println!("  ✓ Cached pool within cap: {}", cached_value);
            }

            // Summary
            println!("\n=== SUMMARY ===");
            if has_issues || overlap_count > 0 || has_bounds_mismatch {
                println!("✗ LazyColumn has rendering issues:");
                println!("  - Overlaps: {}", overlap_count);
                println!("  - Size issues: {}", has_issues);
                println!("  - Bounds mismatch: {}", has_bounds_mismatch);
                robot.exit().ok();
                std::process::exit(1);
            } else {
                println!("✓ LazyColumn rendering looks correct");
            }

            println!("\n=== LazyList Robot Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
