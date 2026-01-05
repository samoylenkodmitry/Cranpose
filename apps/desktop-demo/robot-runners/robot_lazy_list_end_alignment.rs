//! Robot test for LazyList end alignment - validates last item bottom alignment.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_list_end_alignment --features robot-app
//! ```

mod robot_test_utils;

use compose_app::AppLauncher;
use compose_testing::{
    find_button_in_semantics, find_in_semantics, find_text_by_prefix_in_semantics, find_text_exact,
};
use desktop_app::app;
use robot_test_utils::{find_element_by_text_exact, print_semantics_with_bounds, union_bounds};
use std::time::Duration;

fn main() {
    env_logger::init();
    println!("=== LazyList End Alignment Test ===");

    AppLauncher::new()
        .with_title("LazyList End Alignment Test")
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

            println!("\n--- Step 2: Capture item count ---");
            let total_count = find_text_by_prefix_in_semantics(&robot, "Virtualized list with")
                .and_then(|(_, _, _, _, text)| {
                    text.strip_prefix("Virtualized list with")
                        .and_then(|rest| rest.trim().split(' ').next())
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0);
            if total_count == 0 {
                println!("FATAL: Could not parse total item count");
                robot.exit().ok();
                std::process::exit(1);
            }
            let last_index = total_count - 1;
            println!(
                "  Parsed total_count={} last_index={}",
                total_count, last_index
            );

            println!("\n--- Step 3: Jump to end ---");
            if !click_button("End") {
                println!("FATAL: Could not find 'End' button");
                robot.exit().ok();
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(800));

            println!("\n--- Step 4: Capture LazyList bounds and last item bounds ---");
            let semantics = robot.get_semantics().ok();
            let Some(elements) = semantics.as_deref() else {
                println!("  ✗ Failed to fetch semantics");
                robot.exit().ok();
                std::process::exit(1);
            };

            let list_bounds =
                find_element_by_text_exact(elements, "LazyListViewport").map(|elem| {
                    (
                        elem.bounds.x,
                        elem.bounds.y,
                        elem.bounds.width,
                        elem.bounds.height,
                    )
                });
            let Some((list_x, list_y, list_w, list_h)) = list_bounds else {
                println!("  ✗ LazyListViewport semantics not found");
                print_semantics_with_bounds(elements, 0);
                robot.exit().ok();
                std::process::exit(1);
            };
            println!(
                "  LazyListViewport bounds=({:.1},{:.1},{:.1},{:.1})",
                list_x, list_y, list_w, list_h
            );

            let item_text = format!("ItemRow #{}", last_index);
            let row_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, &item_text));
            let hello_text = format!("Hello #{}", last_index);
            let hello_bounds = find_in_semantics(&robot, |elem| find_text_exact(elem, &hello_text));

            let Some(row_bounds) = row_bounds else {
                println!("  ✗ {} not found in semantics after End", item_text);
                if let Some(list_elem) = find_element_by_text_exact(elements, "LazyListViewport") {
                    print_semantics_with_bounds(std::slice::from_ref(list_elem), 1);
                }
                robot.exit().ok();
                std::process::exit(1);
            };

            let group_bounds = union_bounds(row_bounds, hello_bounds);
            println!(
                "  Last item bounds row=({:.1},{:.1},{:.1},{:.1}) group=({:.1},{:.1},{:.1},{:.1})",
                row_bounds.0,
                row_bounds.1,
                row_bounds.2,
                row_bounds.3,
                group_bounds.0,
                group_bounds.1,
                group_bounds.2,
                group_bounds.3
            );

            let list_bottom = list_y + list_h;
            let item_bottom = group_bounds.1 + group_bounds.3;
            let gap = list_bottom - item_bottom;
            println!(
                "  Bottom alignment: list_bottom={:.1} item_bottom={:.1} gap={:.1}",
                list_bottom, item_bottom, gap
            );

            let max_allowed_gap = 8.0;
            if gap.abs() > max_allowed_gap {
                println!(
                    "  ✗ Last item bottom misaligned (|gap| {:.1} > {:.1})",
                    gap.abs(),
                    max_allowed_gap
                );
                if let Some(list_elem) = find_element_by_text_exact(elements, "LazyListViewport") {
                    print_semantics_with_bounds(std::slice::from_ref(list_elem), 1);
                }
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("✓ Last item aligns with viewport bottom");
            println!("\n=== LazyList End Alignment Test Complete ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
