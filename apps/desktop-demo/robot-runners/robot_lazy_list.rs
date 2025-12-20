//! Robot test for LazyList tab - validates item positions, bounds, and rendering
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
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
            
            // Find items with FULL BOUNDS (x, y, width, height)
            let find_visible_items_with_bounds = || {
                let mut items: Vec<(usize, f32, f32, f32, f32)> = Vec::new(); // (index, x, y, w, h)
                for i in 0..20 {
                    let item_text = format!("Item #{}", i);
                    if let Some((x, y, w, h)) = find_text_in_semantics(&robot, &item_text) {
                        items.push((i, x, y, w, h));
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

            // Step 2: Verify tab content
            println!("\n--- Step 2: Verify LazyList content ---");
            verify_text("Lazy List Demo");
            verify_text("100 items");

            // Step 3: Find all visible items with FULL BOUNDS
            println!("\n--- Step 3: Validate item BOUNDS (detecting overlaps) ---");
            let items = find_visible_items_with_bounds();
            
            if items.is_empty() {
                println!("  ✗ CRITICAL: No items found! LazyColumn not rendering.");
                robot.exit().ok();
                std::process::exit(1);
            }
            
            println!("  Found {} visible items:", items.len());
            let mut has_issues = false;
            
            for (idx, x, y, w, h) in &items {
                println!("    Item #{}: pos=({:.1}, {:.1}) size=({:.1} x {:.1})", idx, x, y, w, h);
                
                // Check for suspicious sizes
                if *h < 10.0 {
                    println!("      ⚠️  Height too small! Expected ~40-60px");
                    has_issues = true;
                }
                if *w < 100.0 {
                    println!("      ⚠️  Width too small! Expected near full width");
                    has_issues = true;
                }
                if *h > 200.0 {
                    println!("      ⚠️  Height too large!");
                    has_issues = true;
                }
            }
            
            // Check for overlaps
            println!("\n--- Step 4: Check for OVERLAPPING items ---");
            let mut overlap_count = 0;
            for i in 0..items.len() {
                for j in (i+1)..items.len() {
                    let (idx_a, _xa, ya, _wa, ha) = items[i];
                    let (idx_b, _xb, yb, _wb, _hb) = items[j];
                    
                    // Check if item j starts before item i ends
                    let item_a_bottom = ya + ha;
                    if yb < item_a_bottom {
                        println!("  ⚠️  OVERLAP: Item #{} (bottom={:.1}) overlaps with Item #{} (top={:.1})", 
                            idx_a, item_a_bottom, idx_b, yb);
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
                let (idx_prev, _, y_prev, _, h_prev) = items[i-1];
                let (idx_curr, _, y_curr, _, _) = items[i];
                let gap = y_curr - (y_prev + h_prev);
                if gap < -1.0 {
                    println!("  ⚠️  Negative gap ({:.1}px) between Item #{} and #{}", gap, idx_prev, idx_curr);
                } else if gap > 50.0 {
                    println!("  ⚠️  Large gap ({:.1}px) between Item #{} and #{}", gap, idx_prev, idx_curr);
                } else {
                    println!("  Item #{} -> #{}: gap = {:.1}px", idx_prev, idx_curr, gap);
                }
            }

            // Summary
            println!("\n=== SUMMARY ===");
            if has_issues || overlap_count > 0 {
                println!("✗ LazyColumn has rendering issues:");
                println!("  - Overlaps: {}", overlap_count);
                println!("  - Size issues: {}", has_issues);
            } else {
                println!("✓ LazyColumn rendering looks correct");
            }

            println!("\n=== LazyList Robot Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
