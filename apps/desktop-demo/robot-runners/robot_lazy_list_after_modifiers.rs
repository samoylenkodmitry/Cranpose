//! Robot test for LazyList after navigating through Modifiers Showcase.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list_after_modifiers --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_text_in_semantics};
use desktop_app::app;
use robot_test_utils::{
    find_element_by_text_exact, find_in_subtree_by_text, print_semantics_with_bounds,
};
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList After Modifiers (rect validation) ===");

    AppLauncher::new()
        .with_title("LazyList After Modifiers")
        .with_size(1200, 800)
        .with_headless(true)
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

            println!("\n--- Step 1: Navigate to 'Modifiers Showcase' tab ---");
            if !click_button("Modifiers Showcase") {
                println!("FATAL: Could not find 'Modifiers Showcase' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(300));

            if click_button("Long List (50)") {
                std::thread::sleep(Duration::from_millis(200));
            }

            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "Select Showcase") {
                println!("  Scrolling modifiers showcase content");
                robot
                    .drag(
                        x + w / 2.0,
                        y + h / 2.0 + 220.0,
                        x + w / 2.0,
                        y + h / 2.0 - 220.0,
                    )
                    .ok();
                std::thread::sleep(Duration::from_millis(200));
            }

            println!("  Scrolling back to top before switching tabs");
            for _ in 0..3 {
                robot.drag(600.0, 200.0, 600.0, 700.0).ok();
                std::thread::sleep(Duration::from_millis(150));
            }

            println!("\n--- Step 2: Navigate to 'Lazy List' tab ---");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));

            println!("\n--- Step 3: Validate LazyList item rects ---");
            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to read semantics tree");
                robot.exit().ok();
                std::process::exit(1);
            };

            let list_elem = find_element_by_text_exact(elements, "LazyListViewport");
            let Some(list_elem) = list_elem else {
                println!("  ✗ LazyListViewport semantics not found");
                print_semantics_with_bounds(elements, 0);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            };

            let list_bounds = (
                list_elem.bounds.x,
                list_elem.bounds.y,
                list_elem.bounds.width,
                list_elem.bounds.height,
            );
            println!(
                "  ✓ LazyListViewport bounds=({:.1},{:.1},{:.1},{:.1})",
                list_bounds.0, list_bounds.1, list_bounds.2, list_bounds.3
            );

            let row_elem = find_element_by_text_exact(elements, "ItemRow #0");
            let Some(row_elem) = row_elem else {
                println!("  ✗ ItemRow #0 semantics not found");
                print_semantics_with_bounds(std::slice::from_ref(list_elem), 1);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            };

            let row_bounds = (
                row_elem.bounds.x,
                row_elem.bounds.y,
                row_elem.bounds.width,
                row_elem.bounds.height,
            );

            let item_text = find_in_subtree_by_text(row_elem, "Item #0");
            let height_text = find_in_subtree_by_text(row_elem, "h: 48px");
            let hello_text = find_element_by_text_exact(elements, "Hello #0");

            let mut has_issues = false;
            if let Some(item_text) = item_text {
                println!(
                    "  Item #0 bounds=({:.1},{:.1},{:.1},{:.1})",
                    item_text.bounds.x,
                    item_text.bounds.y,
                    item_text.bounds.width,
                    item_text.bounds.height
                );
            } else {
                println!("  ✗ Missing Item #0 text inside ItemRow #0");
                has_issues = true;
            }

            if let Some(height_text) = height_text {
                println!(
                    "  h: 48px bounds=({:.1},{:.1},{:.1},{:.1})",
                    height_text.bounds.x,
                    height_text.bounds.y,
                    height_text.bounds.width,
                    height_text.bounds.height
                );
            } else {
                println!("  ✗ Missing h: 48px text inside ItemRow #0");
                has_issues = true;
            }

            if let Some(hello_text) = hello_text {
                println!(
                    "  Hello #0 bounds=({:.1},{:.1},{:.1},{:.1})",
                    hello_text.bounds.x,
                    hello_text.bounds.y,
                    hello_text.bounds.width,
                    hello_text.bounds.height
                );
            } else {
                println!("  ✗ Missing Hello #0 text");
                has_issues = true;
            }

            let width_delta = (row_bounds.2 - list_bounds.2).abs();
            if width_delta > 2.0 {
                println!(
                    "  ⚠️  ItemRow #0 width mismatch: row={:.1} list={:.1}",
                    row_bounds.2, list_bounds.2
                );
                has_issues = true;
            }

            if let (Some(item_text), Some(height_text)) = (item_text, height_text) {
                let y_delta = (item_text.bounds.y - height_text.bounds.y).abs();
                if y_delta > 2.0 {
                    println!("  ⚠️  Item #0 and h: 48px not aligned (Δy={:.1})", y_delta);
                    has_issues = true;
                }
                let row_bottom = row_bounds.1 + row_bounds.3;
                if item_text.bounds.y < row_bounds.1 - 1.0
                    || item_text.bounds.y > row_bottom + 1.0
                    || height_text.bounds.y < row_bounds.1 - 1.0
                    || height_text.bounds.y > row_bottom + 1.0
                {
                    println!("  ⚠️  ItemRow #0 children exceed row bounds");
                    has_issues = true;
                }
            }

            if let Some(hello_text) = hello_text {
                let row_bottom = row_bounds.1 + row_bounds.3;
                if hello_text.bounds.y < row_bottom - 1.0 {
                    println!("  ⚠️  Hello #0 is not below ItemRow #0");
                    has_issues = true;
                }
            }

            if has_issues {
                println!("\n--- LazyList semantics dump ---");
                print_semantics_with_bounds(std::slice::from_ref(list_elem), 1);
                let _ = robot.send_key("d");
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("✓ LazyList row layout looks correct");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
