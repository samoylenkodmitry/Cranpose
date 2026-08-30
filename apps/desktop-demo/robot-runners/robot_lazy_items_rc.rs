mod robot_launch;

use std::{rc::Rc, time::Duration};

use cranpose_foundation::lazy::{rememberLazyListState, LazyListScopeExt, LazyListState};
use cranpose_macros::composable;
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::{
    widgets::*, Color, ColumnSpec, LinearArrangement, Modifier, RowSpec, TextStyle,
    VerticalAlignment,
};

#[derive(Clone, Debug, PartialEq)]
struct TestItem {
    id: usize,
    name: String,
}

fn lazy_list_with_rc(state: LazyListState, data: Rc<[TestItem]>) {
    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(300.0)
            .background(Color(0.05, 0.05, 0.1, 1.0))
            .rounded_corners(12.0),
        state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(4.0))
            .content_padding(8.0, 8.0),
        |scope| {
            scope.items_slice_rc(data, |item| {
                rc_item_content(item);
            });
        },
    );
}

fn lazy_list_with_indexed_rc(state: LazyListState, data: Rc<[TestItem]>) {
    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .height(300.0)
            .background(Color(0.08, 0.05, 0.1, 1.0))
            .rounded_corners(12.0),
        state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(4.0))
            .content_padding(8.0, 8.0),
        |scope| {
            scope.items_indexed_rc(data, |index, item| {
                indexed_rc_item_content(index, item);
            });
        },
    );
}

fn rc_item_content(item: &TestItem) {
    let id = item.id;
    let name = item.name.clone();
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .background(Color(0.12, 0.16, 0.28, 0.9))
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                format!("RcItem #{}", id),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            Text(
                name.clone(),
                Modifier::empty()
                    .padding(4.0)
                    .background(Color(0.0, 0.3, 0.0, 0.5))
                    .rounded_corners(4.0),
                TextStyle::default(),
            );
        },
    );
}

fn indexed_rc_item_content(index: usize, item: &TestItem) {
    let id = item.id;
    let name = item.name.clone();
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .background(Color(0.16, 0.12, 0.28, 0.9))
            .rounded_corners(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                format!("IdxRcItem[{}]", index),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            Text(
                format!("#{} {}", id, name),
                Modifier::empty()
                    .padding(4.0)
                    .background(Color(0.3, 0.0, 0.3, 0.5))
                    .rounded_corners(4.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn rc_items_test_app() {
    let data: Rc<[TestItem]> = Rc::from(
        (0..15)
            .map(|i| TestItem {
                id: i,
                name: format!("Item-{}", i),
            })
            .collect::<Vec<_>>(),
    );

    let state1 = rememberLazyListState();
    let state2 = rememberLazyListState();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.08, 0.08, 0.12, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "LazyList Rc Items Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(0.2, 0.3, 0.5, 0.8))
                    .rounded_corners(8.0),
                TextStyle::default(),
            );

            Text(
                "items_slice_rc (zero-copy):",
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            lazy_list_with_rc(state1, Rc::clone(&data));

            Text(
                "items_indexed_rc (zero-copy with index):",
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            lazy_list_with_indexed_rc(state2, Rc::clone(&data));
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== LazyList Rc Items Robot Test ===");

    robot_launch::launch("Rc Items Test", 800, 800)
        .with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(300));
            let _ = robot.wait_for_idle();

            let mut errors = Vec::new();

            println!("\n--- Step 1: Verify app loaded ---");
            if find_text_in_semantics(&robot, "LazyList Rc Items Test").is_some() {
                println!("  ✓ Title found");
            } else {
                println!("  ✗ Title not found!");
                errors.push("Title not found");
            }

            println!("\n--- Step 2: Verify items_slice_rc items ---");
            let mut rc_items_found = 0;
            for i in 0..10 {
                let item_text = format!("RcItem #{}", i);
                if find_text_in_semantics(&robot, &item_text).is_some() {
                    println!("  ✓ Found '{}'", item_text);
                    rc_items_found += 1;
                }
            }
            if rc_items_found < 3 {
                errors.push("items_slice_rc: Not enough items visible");
                println!(
                    "  ✗ Only {} items found, expected at least 3",
                    rc_items_found
                );
            } else {
                println!(
                    "  ✓ items_slice_rc rendered {} items correctly",
                    rc_items_found
                );
            }

            println!("\n--- Step 3: Verify items_indexed_rc items ---");
            let mut indexed_items_found = 0;
            for i in 0..10 {
                let item_text = format!("IdxRcItem[{}]", i);
                if find_text_in_semantics(&robot, &item_text).is_some() {
                    println!("  ✓ Found '{}'", item_text);
                    indexed_items_found += 1;
                }
            }
            if indexed_items_found < 3 {
                errors.push("items_indexed_rc: Not enough items visible");
                println!(
                    "  ✗ Only {} items found, expected at least 3",
                    indexed_items_found
                );
            } else {
                println!(
                    "  ✓ items_indexed_rc rendered {} items correctly",
                    indexed_items_found
                );
            }

            println!("\n--- Step 4: Verify item data access ---");
            let mut name_found = false;
            for i in 0..5 {
                let name_text = format!("Item-{}", i);
                if find_text_in_semantics(&robot, &name_text).is_some() {
                    println!("  ✓ Found item name '{}'", name_text);
                    name_found = true;
                    break;
                }
            }
            if !name_found {
                errors.push("Item names not rendered - data access may be broken");
                println!("  ✗ No item names found!");
            }

            println!("\n--- Step 5: Scroll test ---");
            if let Some((x, y, w, h)) = find_text_in_semantics(&robot, "RcItem #0") {
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

                let mut new_items_found = false;
                for i in 5..10 {
                    let item_text = format!("RcItem #{}", i);
                    if find_text_in_semantics(&robot, &item_text).is_some() {
                        println!("  ✓ After scroll: found '{}'", item_text);
                        new_items_found = true;
                        break;
                    }
                }
                if new_items_found {
                    println!("  ✓ Scroll revealed new items");
                } else {
                    println!("  ⚠️ Scroll may not have revealed new items");
                }
            }

            println!("\n=== SUMMARY ===");
            if errors.is_empty() {
                println!("✓ All Rc-based item tests PASSED!");
                println!("  - items_slice_rc: {} items rendered", rc_items_found);
                println!(
                    "  - items_indexed_rc: {} items rendered",
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
        .run(rc_items_test_app);
}
