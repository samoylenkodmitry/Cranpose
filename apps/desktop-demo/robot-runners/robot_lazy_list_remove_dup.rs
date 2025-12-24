//! Robot test for LazyList after removing items - validates duplicate layouts.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list_remove_dup --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use robot_test_utils::{
    collect_by_text_exact, collect_text_prefix_counts, print_semantics_with_bounds,
};
use std::collections::HashMap;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList Remove 10 Duplication Test ===");

    AppLauncher::new()
        .with_title("LazyList Remove 10 Dup Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(150));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };
            let click_button_or_text = |name: &str| -> bool {
                if click_button(name) {
                    return true;
                }
                if let Some((x, y, w, h)) = find_text_in_semantics(&robot, name) {
                    println!("  Found text '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(150));
                    return true;
                }
                false
            };

            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if !click_button_or_text("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            println!("\n--- Step 2: Click 'Remove 10' ---");
            if !click_button_or_text("Remove 10") {
                println!("FATAL: Could not find 'Remove 10' button");
                if let Ok(elements) = robot.get_semantics() {
                    print_semantics_with_bounds(&elements, 0);
                }
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(400));

            if find_text_in_semantics(&robot, "Virtualized list with 90 items").is_none() {
                println!("  ⚠️  Item count text did not update to 90");
            } else {
                println!("  ✓ Item count updated to 90");
            }

            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to read semantics tree");
                robot.exit().ok();
                std::process::exit(1);
            };

            println!("\n--- Step 3: Count LazyListViewport occurrences ---");
            let mut viewports = Vec::new();
            collect_by_text_exact(elements, "LazyListViewport", &mut viewports);
            println!("  Found LazyListViewport count: {}", viewports.len());
            for (idx, elem) in viewports.iter().enumerate() {
                println!(
                    "  Viewport #{} bounds=({:.1},{:.1},{:.1},{:.1})",
                    idx + 1,
                    elem.bounds.x,
                    elem.bounds.y,
                    elem.bounds.width,
                    elem.bounds.height
                );
            }

            println!("\n--- Step 4: Check for duplicate item rows ---");
            let mut row_counts: HashMap<String, usize> = HashMap::new();
            collect_text_prefix_counts(elements, "ItemRow #", &mut row_counts);
            let mut duplicates: Vec<(String, usize)> = row_counts
                .into_iter()
                .filter(|(_, count)| *count > 1)
                .collect();
            duplicates.sort_by(|a, b| a.0.cmp(&b.0));

            if !duplicates.is_empty() || viewports.len() != 1 {
                println!("  ✗ Detected duplicate LazyList layout artifacts");
                if !duplicates.is_empty() {
                    for (label, count) in &duplicates {
                        println!("    Duplicate {} -> {} occurrences", label, count);
                    }
                }
                print_semantics_with_bounds(elements, 0);
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("✓ No duplicate LazyList layout detected");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
