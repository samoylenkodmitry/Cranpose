use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;

fn main() {
    env_logger::init();
    println!("=== Comprehensive LazyList Tab Robot Test ===\n");

    AppLauncher::new()
        .with_title("LazyList Tab Test")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            println!("✓ App launched\n");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                } else if let Some((x, y, w, h)) = find_text_in_semantics(&robot, name) {
                    println!("  Found text/button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    return true;
                }
                println!("  ✗ Button '{}' not found!", name);
                false
            };

            let find_text = |text: &str| -> Option<(f32, f32, f32, f32)> {
                find_text_in_semantics(&robot, text)
            };

            let find_visible_items = || -> Vec<usize> {
                let mut items = Vec::new();
                for i in 0..30 {
                    let item_text = format!("Item #{}", i);
                    if find_text(&item_text).is_some() {
                        items.push(i);
                    }
                }
                for offset in 0..20 {
                    let mid = usize::MAX / 2;
                    let idx = mid.saturating_sub(10).saturating_add(offset);
                    let item_text = format!("Item #{}", idx);
                    if find_text(&item_text).is_some() {
                        items.push(idx);
                    }
                }
                items
            };

            let get_item_count_text = || -> Option<String> {
                for count in [100usize, 10, 1000] {
                    let text = format!("Virtualized list with {} items", count);
                    if find_text(&text).is_some() {
                        return Some(format!("{} items", count));
                    }
                }
                let huge_text = format!("Virtualized list with {} items", usize::MAX);
                if find_text(&huge_text).is_some() {
                    return Some(format!("{} items (usize::MAX)", usize::MAX));
                }
                if find_text("Virtualized list").is_some() {
                    return Some("(found 'Virtualized list' text)".to_string());
                }
                None
            };

            println!("=== PHASE 0: Navigate to Lazy List Tab ===");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find Lazy List tab!");
                robot.exit().ok();
                return;
            }
            std::thread::sleep(Duration::from_millis(400));

            if find_text("Lazy List Demo").is_some() {
                println!("  ✓ Lazy List Demo tab loaded");
            } else {
                println!("  ✗ Lazy List Demo NOT loaded!");
            }

            println!("\n=== PHASE 1: Initial State ===");

            if let Some(count_text) = get_item_count_text() {
                println!("  Count text: {}", count_text);
            } else {
                println!("  ✗ Count text not found");
            }

            let initial_items = find_visible_items();
            println!("  Visible items: {:?}", initial_items);
            println!("  Total visible: {}", initial_items.len());

            println!("\n=== PHASE 2: Scroll List ===");

            if let Some((_, y, _, _)) = find_text("Item #0") {
                let start_y = y + 100.0;
                let end_y = y - 100.0;
                println!("  Scrolling from y={:.0} to y={:.0}", start_y, end_y);
                robot.drag(400.0, start_y, 400.0, end_y).ok();
                std::thread::sleep(Duration::from_millis(300));
            }

            let after_scroll_items = find_visible_items();
            println!("  After scroll visible: {:?}", after_scroll_items);

            if after_scroll_items != initial_items {
                println!("  ✓ Scroll changed visible items");
            } else {
                println!("  ⚠️ Scroll may not have worked");
            }

            println!("\n=== PHASE 3: Click 'Set usize::MAX' ===");

            if click_button("Set usize::MAX") {
                println!("  ✓ Clicked 'Set usize::MAX'");
                std::thread::sleep(Duration::from_millis(500));

                if find_text("Lazy List Demo").is_some() {
                    println!("  ✓ App still responsive!");
                } else {
                    println!("  ✗ APP MAY HAVE CRASHED!");
                    robot.exit().ok();
                    return;
                }

                if let Some(count_text) = get_item_count_text() {
                    println!("  Count text after MAX: {}", count_text);
                }

                let max_items = find_visible_items();
                println!("  Visible items after MAX: {} items", max_items.len());
                if !max_items.is_empty() {
                    println!("  First visible: Item #{}", max_items[0]);
                }
            } else {
                println!("  ✗ 'Set usize::MAX' button not found!");
            }

            println!("\n=== PHASE 4: Click 'Jump to Middle' ===");

            if click_button("Jump to Middle") {
                println!("  ✓ Clicked 'Jump to Middle'");
                std::thread::sleep(Duration::from_millis(500));

                if find_text("Lazy List Demo").is_some() {
                    println!("  ✓ App still responsive!");
                } else {
                    println!("  ✗ APP MAY HAVE CRASHED!");
                    robot.exit().ok();
                    return;
                }

                if let Some(count_text) = get_item_count_text() {
                    println!("  Count text after jump: {}", count_text);
                }

                let middle_items = find_visible_items();
                println!("  Visible items after jump: {} items", middle_items.len());

                let mid = usize::MAX / 2;
                let mut found_middle = false;
                for &idx in &middle_items {
                    if idx > mid.saturating_sub(20) && idx < mid.saturating_add(20) {
                        found_middle = true;
                        break;
                    }
                }
                if found_middle {
                    println!("  ✓ Jumped to middle: visible near index {}", mid);
                } else if middle_items.is_empty() {
                    println!("  ⚠️ No items visible (may need to check semantics)");
                } else {
                    println!("  ⚠️ Items visible but not near expected middle");
                    println!("  First visible: Item #{}", middle_items[0]);
                }
            } else {
                println!("  ✗ 'Jump to Middle' button not found!");
            }

            println!("\n=== SUMMARY ===");
            let success = find_text("Lazy List Demo").is_some();

            if success {
                println!("✓ LazyList tab test PASSED - app stable");
            } else {
                println!("✗ Test FAILED - app crashed");
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
