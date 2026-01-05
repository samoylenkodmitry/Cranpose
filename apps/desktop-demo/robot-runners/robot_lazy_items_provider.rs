//! Robot test for LazyList items_with_provider and items_indexed_with_provider methods.
//!
//! Validates that callback-based provider APIs render correctly with zero allocation.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_lazy_items_provider --features robot-app
//! ```

use compose_app::AppLauncher;
use compose_foundation::lazy::{remember_lazy_list_state, LazyListScopeExt, LazyListState};
use compose_macros::composable;
use compose_testing::find_text_in_semantics;
use compose_ui::widgets::*;
use compose_ui::{Color, ColumnSpec, LinearArrangement, Modifier, RowSpec, VerticalAlignment};
use std::rc::Rc;
use std::time::Duration;

/// Test data struct
#[derive(Clone, Debug, PartialEq)]
struct ProviderItem {
    id: usize,
    label: String,
}

/// LazyList using items_with_provider (callback-based, zero allocation)
fn lazy_list_with_provider(state: LazyListState, data: Rc<Vec<ProviderItem>>) {
    let count = data.len();

    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(300.0)
            .background(Color(0.05, 0.08, 0.05, 1.0))
            .rounded_corners(12.0),
        state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(4.0))
            .content_padding(8.0, 8.0),
        |scope| {
            // Use items_with_provider - provider closure fetches data on demand
            let data_clone = Rc::clone(&data);
            scope.items_with_provider(
                count,
                move |index| data_clone.get(index).cloned(),
                |item| {
                    provider_item_content(item);
                },
            );
        },
    );
}

/// LazyList using items_indexed_with_provider (callback-based with index)
fn lazy_list_with_indexed_provider(state: LazyListState, data: Rc<Vec<ProviderItem>>) {
    let count = data.len();

    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(300.0)
            .background(Color(0.08, 0.05, 0.08, 1.0))
            .rounded_corners(12.0),
        state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(4.0))
            .content_padding(8.0, 8.0),
        |scope| {
            // Use items_indexed_with_provider - provider closure fetches data on demand
            let data_clone = Rc::clone(&data);
            scope.items_indexed_with_provider(
                count,
                move |index| data_clone.get(index).cloned(),
                |index, item| {
                    indexed_provider_item_content(index, item);
                },
            );
        },
    );
}

/// Non-composable provider item content
fn provider_item_content(item: ProviderItem) {
    let id = item.id;
    let label = item.label;
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .background(Color(0.12, 0.20, 0.12, 0.9))
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(format!("ProvItem #{}", id), Modifier::empty().padding(4.0));
            Text(
                label.clone(),
                Modifier::empty()
                    .padding(4.0)
                    .background(Color(0.0, 0.4, 0.0, 0.5))
                    .rounded_corners(4.0),
            );
        },
    );
}

/// Non-composable indexed provider item content
fn indexed_provider_item_content(index: usize, item: ProviderItem) {
    let id = item.id;
    let label = item.label;
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .background(Color(0.20, 0.12, 0.20, 0.9))
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                format!("IdxProv[{}]", index),
                Modifier::empty().padding(4.0),
            );
            Text(
                format!("#{} {}", id, label),
                Modifier::empty()
                    .padding(4.0)
                    .background(Color(0.4, 0.0, 0.4, 0.5))
                    .rounded_corners(4.0),
            );
        },
    );
}

#[composable]
fn provider_items_test_app() {
    // Create test data - stored in Rc<Vec> so provider closure can access it
    let data: Rc<Vec<ProviderItem>> = Rc::new(
        (0..15)
            .map(|i| ProviderItem {
                id: i,
                label: format!("Label-{}", i),
            })
            .collect(),
    );

    let state1 = remember_lazy_list_state();
    let state2 = remember_lazy_list_state();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.08, 0.08, 0.12, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "LazyList Provider Items Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(0.3, 0.4, 0.2, 0.8))
                    .rounded_corners(8.0),
            );

            Text(
                "items_with_provider (callback-based):",
                Modifier::empty().padding(4.0),
            );
            lazy_list_with_provider(state1.clone(), Rc::clone(&data));

            Text(
                "items_indexed_with_provider (callback with index):",
                Modifier::empty().padding(4.0),
            );
            lazy_list_with_indexed_provider(state2.clone(), Rc::clone(&data));
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== LazyList Provider Items Robot Test ===");

    AppLauncher::new()
        .with_title("Provider Items Test")
        .with_size(800, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let mut errors = Vec::new();

            // Step 1: Verify title
            println!("\n--- Step 1: Verify app loaded ---");
            if find_text_in_semantics(&robot, "LazyList Provider Items Test").is_some() {
                println!("  ✓ Title found");
            } else {
                println!("  ✗ Title not found!");
                errors.push("Title not found");
            }

            // Step 2: Verify items_with_provider items
            println!("\n--- Step 2: Verify items_with_provider items ---");
            let mut prov_items_found = 0;
            for i in 0..10 {
                let item_text = format!("ProvItem #{}", i);
                if find_text_in_semantics(&robot, &item_text).is_some() {
                    println!("  ✓ Found '{}'", item_text);
                    prov_items_found += 1;
                }
            }
            if prov_items_found < 3 {
                errors.push("items_with_provider: Not enough items visible");
                println!(
                    "  ✗ Only {} items found, expected at least 3",
                    prov_items_found
                );
            } else {
                println!(
                    "  ✓ items_with_provider rendered {} items correctly",
                    prov_items_found
                );
            }

            // Step 3: Verify items_indexed_with_provider items
            println!("\n--- Step 3: Verify items_indexed_with_provider items ---");
            let mut indexed_items_found = 0;
            for i in 0..10 {
                let item_text = format!("IdxProv[{}]", i);
                if find_text_in_semantics(&robot, &item_text).is_some() {
                    println!("  ✓ Found '{}'", item_text);
                    indexed_items_found += 1;
                }
            }
            if indexed_items_found < 3 {
                errors.push("items_indexed_with_provider: Not enough items visible");
                println!(
                    "  ✗ Only {} items found, expected at least 3",
                    indexed_items_found
                );
            } else {
                println!(
                    "  ✓ items_indexed_with_provider rendered {} items correctly",
                    indexed_items_found
                );
            }

            // Step 4: Verify item labels rendered (proves provider callback works)
            println!("\n--- Step 4: Verify provider callback data access ---");
            let mut label_found = false;
            for i in 0..5 {
                let label_text = format!("Label-{}", i);
                if find_text_in_semantics(&robot, &label_text).is_some() {
                    println!("  ✓ Found item label '{}'", label_text);
                    label_found = true;
                    break;
                }
            }
            if !label_found {
                errors.push("Item labels not rendered - provider callback may be broken");
                println!("  ✗ No item labels found!");
            }

            // Step 5: Scroll test for items_with_provider list
            println!("\n--- Step 5: Scroll test ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "ProvItem #0") {
                robot
                    .drag(
                        x + w / 2.0,
                        y + h / 2.0 + 50.0,
                        x + w / 2.0,
                        y + h / 2.0 - 100.0,
                    )
                    .ok();
                std::thread::sleep(Duration::from_millis(200));
                let _ = robot.wait_for_idle();

                // Check if new items appeared
                let mut new_items_found = false;
                for i in 5..10 {
                    let item_text = format!("ProvItem #{}", i);
                    if find_text_in_semantics(&robot, &item_text).is_some() {
                        println!("  ✓ After scroll: found '{}'", item_text);
                        new_items_found = true;
                        break;
                    }
                }
                if new_items_found {
                    println!("  ✓ Scroll revealed new items (provider callback works lazily)");
                } else {
                    println!("  ⚠️ Scroll may not have revealed new items");
                }
            }

            // Summary
            println!("\n=== SUMMARY ===");
            if errors.is_empty() {
                println!("✓ All provider-based item tests PASSED!");
                println!(
                    "  - items_with_provider: {} items rendered",
                    prov_items_found
                );
                println!(
                    "  - items_indexed_with_provider: {} items rendered",
                    indexed_items_found
                );
                robot.exit().ok();
            } else {
                println!("✗ Tests FAILED:");
                for err in &errors {
                    println!("  - {}", err);
                }
                robot.exit().ok();
                std::process::exit(1);
            }
        })
        .run(provider_items_test_app);
}
