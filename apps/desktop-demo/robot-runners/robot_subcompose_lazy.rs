mod robot_launch;

use std::time::Duration;

use cranpose::LazyItems;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::{widgets::*, Modifier, TextStyle};

fn test_app() {
    use cranpose_ui::{Color, LinearArrangement};

    let list_state = rememberLazyListState();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(20.0)
            .background(Color(0.1, 0.1, 0.1, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                "SubcomposeLayout Test".to_string(),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Text(
                "20 test items".to_string(),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );

            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(300.0)
                    .background(Color(0.05, 0.05, 0.1, 1.0)),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                |scope| {
                    scope.items(LazyItems::new(20).key(|i: usize| i as u64), move |i| {
                        let bg = if i % 2 == 0 {
                            Color(0.2, 0.25, 0.3, 1.0)
                        } else {
                            Color(0.15, 0.18, 0.22, 1.0)
                        };

                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .height(50.0)
                                .padding(10.0)
                                .background(bg),
                            RowSpec::new().horizontal_arrangement(LinearArrangement::SpaceBetween),
                            move || {
                                Text(
                                    format!("TestItem{}", i),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                                Text(
                                    format!("val={}", i * 10),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    });
                },
            );

            let stats = list_state.stats();
            Text(
                format!(
                    "Visible: {} | Cached: {}",
                    stats.items_in_use, stats.items_in_pool
                ),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

fn main() {
    env_logger::init();
    println!("=== SubcomposeLayout & LazyColumn Comprehensive Test ===\n");

    robot_launch::launch("SubcomposeLayout Test", 800, 600).with_test_driver(|robot| {
            println!("✓ App launched");
            std::thread::sleep(Duration::from_millis(500));

            println!("\n=== PHASE 1: Basic Rendering ===");

            let find_text = |text: &str| -> Option<(f32, f32, f32, f32)> {
                find_text_in_semantics(&robot, text)
            };

            if find_text("SubcomposeLayout Test").is_some() {
                println!("  ✓ Header rendered");
            } else {
                println!("  ✗ Header NOT found!");
            }

            println!("\n=== PHASE 2: Initial Item Positions & Sizes ===");

            let mut visible_items: Vec<(usize, f32, f32, f32, f32)> = Vec::new();

            for i in 0..20 {
                let item_text = format!("TestItem{}", i);
                if let Some((x, y, w, h)) = find_text(&item_text) {
                    visible_items.push((i, x, y, w, h));
                    println!("  Item {}: pos=({:.0}, {:.0}) size=({:.0}x{:.0})", i, x, y, w, h);
                }
            }

            let visible_count = visible_items.len();
            println!("\n  Visible items: {}", visible_count);

            if visible_count < 20 {
                println!("  ✓ Virtualization working: {} items visible (not all 20)", visible_count);
            } else {
                println!("  ✗ Virtualization FAILED: all 20 items visible");
            }

            println!("\n=== PHASE 3: Position Order & Spacing ===");

            let mut all_ordered = true;
            let mut spacing_issues = 0;

            for i in 1..visible_items.len() {
                let (idx_prev, _, y_prev, _, h_prev) = visible_items[i-1];
                let (idx_curr, _, y_curr, _, _) = visible_items[i];

                if y_curr <= y_prev {
                    println!("  ✗ Order violation: Item {} at y={:.0} should be after Item {} at y={:.0}",
                        idx_curr, y_curr, idx_prev, y_prev);
                    all_ordered = false;
                }

                let gap = y_curr - (y_prev + h_prev);
                if gap < 0.0 {
                    println!("  ✗ OVERLAP between Item {} and {}: gap={:.1}px", idx_prev, idx_curr, gap);
                    spacing_issues += 1;
                } else if gap > 50.0 {
                    println!("  ⚠️ Large gap between Item {} and {}: {:.1}px", idx_prev, idx_curr, gap);
                } else {
                    println!("  Gap {}->{}: {:.1}px (expected ~34px)", idx_prev, idx_curr, gap);
                }
            }

            if all_ordered {
                println!("  ✓ All items in correct Y order");
            }
            if spacing_issues == 0 {
                println!("  ✓ No overlapping items");
            }

            println!("\n=== PHASE 4: Scroll Behavior ===");

            let first_before = visible_items.first().map(|(i, _, _, _, _)| *i);
            println!("  First visible before scroll: Item {:?}", first_before);

            let scroll_start_y = visible_items.first().map(|(_, _, y, _, _)| y + 100.0).unwrap_or(400.0);
            robot.drag(400.0, scroll_start_y, 400.0, scroll_start_y - 200.0).ok();
            std::thread::sleep(Duration::from_millis(300));
            println!("  Performed scroll gesture (200px down)");

            let mut items_after: Vec<(usize, f32, f32, f32, f32)> = Vec::new();
            for i in 0..20 {
                let item_text = format!("TestItem{}", i);
                if let Some((x, y, w, h)) = find_text(&item_text) {
                    items_after.push((i, x, y, w, h));
                }
            }

            let first_after = items_after.first().map(|(i, _, _, _, _)| *i);
            println!("  First visible after scroll: Item {:?}", first_after);

            match (first_before, first_after) {
                (Some(before), Some(after)) if after > before => {
                    println!("  ✓ Scroll worked: first item changed from {} to {}", before, after);
                }
                (Some(_), Some(_)) => {
                    let pos_before = visible_items.first().map(|(_, _, y, _, _)| *y);
                    let pos_after = items_after.first().map(|(_, _, y, _, _)| *y);
                    if pos_before != pos_after {
                        println!("  ✓ Scroll worked: item positions changed");
                    } else {
                        println!("  ✗ Scroll may have failed: no change detected");
                    }
                }
                _ => {
                    println!("  ✗ Could not compare scroll states");
                }
            }

            println!("\n=== PHASE 5: Virtualization Stats ===");

            if let Some((_, y, _, _)) = find_text("Visible:") {
                println!("  Stats found at y={:.0}", y);
            }

            let high_items_visible: Vec<_> = items_after.iter().filter(|(i, _, _, _, _)| *i >= 15).collect();
            if !high_items_visible.is_empty() {
                println!("  Items 15+ visible after scroll (expected): {:?}",
                    high_items_visible.iter().map(|(i, _, _, _, _)| i).collect::<Vec<_>>());
            }

            println!("\n=== SUMMARY ===");
            let passed = visible_count < 20 && all_ordered && spacing_issues == 0;
            if passed {
                println!("✓ All SubcomposeLayout/LazyColumn tests PASSED");
            } else {
                println!("✗ Some tests FAILED:");
                if visible_count >= 20 { println!("  - Virtualization broken"); }
                if !all_ordered { println!("  - Item ordering broken"); }
                if spacing_issues > 0 { println!("  - {} overlapping items", spacing_issues); }
            }

            println!("\n=== Test Complete ===");
            robot.exit().ok();
        })
        .run(test_app);
}
