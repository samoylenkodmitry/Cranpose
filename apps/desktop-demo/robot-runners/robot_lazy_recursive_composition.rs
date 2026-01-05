//! Robot test for LazyList recursive composition bug - validates children aren't placed separately.
//!
//! This test catches a bug where lazy list item children (e.g., Text inside Row) are being
//! measured and placed as separate lazy list items, instead of being laid out by their parent.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_recursive_composition --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{find_button_in_semantics, find_in_semantics, find_text_exact};
use desktop_app::app;
use robot_test_utils::find_element_by_text_exact;
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList Recursive Composition Bug Test ===");

    AppLauncher::new()
        .with_title("LazyList Recursive Composition Test")
        .with_size(1200, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            let click_button = |name: &str| -> bool {
                if let Some((x, y, w, h)) = find_button_in_semantics(&robot, name) {
                    println!("  Found button '{}' at ({:.1}, {:.1})", name, x, y);
                    robot.click(x + w / 2.0, y + h / 2.0).ok();
                    std::thread::sleep(Duration::from_millis(200));
                    true
                } else {
                    println!("  ✗ Button '{}' not found!", name);
                    false
                }
            };

            println!("\n--- Step 1: Navigate to 'Lazy List' tab ---");
            if !click_button("Lazy List") {
                println!("FATAL: Could not find 'Lazy List' tab button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(500));

            println!("\n--- Step 2: Validate item children are nested, not separate ---");
            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to fetch semantics");
                robot.exit().ok();
                std::process::exit(1);
            };

            // Find the LazyListViewport
            let Some(list_elem) = find_element_by_text_exact(elements, "LazyListViewport") else {
                println!("  ✗ LazyListViewport not found");
                robot.exit().ok();
                std::process::exit(1);
            };

            // Get bounds of ItemRow #0 - this is the parent container for the first item
            let row_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, "ItemRow #0"));
            let Some((row_x, row_y, row_w, row_h)) = row_bounds else {
                println!("  ✗ ItemRow #0 not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!("  ItemRow #0 bounds: ({:.1}, {:.1}, {:.1}, {:.1})", row_x, row_y, row_w, row_h);

            // Get bounds of "Item #0" text - this should be INSIDE ItemRow #0
            let text_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, "Item #0"));
            let Some((text_x, text_y, _text_w, _text_h)) = text_bounds else {
                println!("  ✗ 'Item #0' text not found");
                robot.exit().ok();
                std::process::exit(1);
            };
            println!("  'Item #0' text at: ({:.1}, {:.1})", text_x, text_y);

            // BUG CHECK: If children are being placed separately, "Item #0" text would appear
            // at its own offset in the list, NOT inside ItemRow #0's bounds.
            // The text should be contained within the row's bounds.
            let text_inside_row = text_y >= row_y && text_y < row_y + row_h;
            if !text_inside_row {
                println!("  ✗ BUG DETECTED: 'Item #0' text is NOT inside ItemRow #0!");
                println!("    Text Y={:.1} should be between Row Y={:.1} and Y+H={:.1}", 
                         text_y, row_y, row_y + row_h);
                println!("  This indicates children are being placed as separate lazy list items.");
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ 'Item #0' text is correctly inside ItemRow #0");

            // Additional check: Look for duplicate children outside their parent
            // If there's an "Item #0" text AND an ItemRow #0, and Item #0 is outside the row bounds
            // but at a different position, that would indicate the bug.

            // Count how many direct children of LazyListViewport exist
            let direct_children_count = list_elem.children.len();
            println!("  LazyListViewport has {} direct children", direct_children_count);

            // In the buggy case, there would be many more direct children (all the nested elements)
            // In the correct case, only the root rows should be direct children (around 6-10)
            if direct_children_count > 50 {
                println!("  ✗ BUG: Too many direct children ({})!", direct_children_count);
                println!("    Expected ~10 root items, got {} - suggests nested children are placed separately",
                         direct_children_count);
                robot.exit().ok();
                std::process::exit(1);
            }
            println!("  ✓ Direct children count ({}) is reasonable", direct_children_count);

            println!("\n✓ No recursive composition bug detected");
            println!("=== LazyList Recursive Composition Test PASSED ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
