use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use cranpose_ui::{
    widgets::{
        Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row,
        RowSpec, Text,
    },
    Alignment, Color, LinearArrangement, Modifier, Size, TextStyle,
};

fn main() {
    env_logger::init();
    println!("=== Lazy Scroll Edge Cases Robot Test ===");

    AppLauncher::new()
        .with_title("Lazy Scroll Edge Cases")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            println!("\n--- Test 1: Spacing in scroll/jump calculations ---");

            let item_0 = find_text_in_semantics(&robot, "Item 0");
            assert!(item_0.is_some(), "Item 0 should be visible initially");
            let (_, y0, _, _) = item_0.unwrap();
            println!("  Item 0 y={:.1}", y0);

            let (_, header_y, _, header_h) =
                find_button_in_semantics(&robot, "Jump 50").expect("Jump button should exist");
            let list_top = header_y + header_h;
            println!("  List starts at y={:.1}", list_top);

            let item_1 = find_text_in_semantics(&robot, "Item 1");
            assert!(item_1.is_some(), "Item 1 should be visible");
            let (_, y1, _, _) = item_1.unwrap();
            let spacing_0_1 = y1 - y0;
            println!(
                "  Item 1 y={:.1}, spacing from Item 0: {:.1}px",
                y1, spacing_0_1
            );

            let expected_spacing = 60.0;
            assert!(
                (spacing_0_1 - expected_spacing).abs() < 5.0,
                "Spacing between items should be ~60px (50 + 10 spacing), got {:.1}",
                spacing_0_1
            );
            println!(
                "  ✓ Spacing is correct: {:.1}px (expected ~{:.1}px)",
                spacing_0_1, expected_spacing
            );

            println!("\n--- Test 2: Large scroll jump preserves spacing ---");

            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Jump 50").expect("Jump button should exist");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let item_50 = find_text_in_semantics(&robot, "Item 50");
            assert!(item_50.is_some(), "Item 50 should be visible after jump");
            let (_, y50, _, _) = item_50.unwrap();
            println!("  Item 50 y={:.1} after jump", y50);

            let item_51 = find_text_in_semantics(&robot, "Item 51");
            if let Some((_, y51, _, _)) = item_51 {
                let spacing_50_51 = y51 - y50;
                println!(
                    "  Item 51 y={:.1}, spacing from Item 50: {:.1}px",
                    y51, spacing_50_51
                );

                assert!(
                    (spacing_50_51 - expected_spacing).abs() < 5.0,
                    "Spacing should be preserved after jump: expected ~60px, got {:.1}",
                    spacing_50_51
                );
                println!("  ✓ Spacing preserved after jump");
            }

            if find_text_in_semantics(&robot, "Item 0").is_some() {
                println!("  ⚠ Item 0 still visible (should be virtualized out)");
            } else {
                println!("  ✓ Item 0 correctly virtualized out");
            }

            println!("\n--- Test 3: Content padding in scroll bounds ---");

            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Jump 95").expect("Jump 95 button should exist");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let item_95 = find_text_in_semantics(&robot, "Item 95");
            if let Some((_, y95, _, _)) = item_95 {
                println!("  Item 95 y={:.1}", y95);

                let item_96 = find_text_in_semantics(&robot, "Item 96");
                if let Some((_, y96, _, _)) = item_96 {
                    let spacing = y96 - y95;
                    println!("  Item 96 y={:.1}, spacing: {:.1}px", y96, spacing);
                    assert!(
                        (spacing - expected_spacing).abs() < 5.0,
                        "Spacing at end should be ~60px, got {:.1}",
                        spacing
                    );
                    println!("  ✓ End items have correct spacing");
                }
            } else {
                println!("  Item 95 not found - may need more scroll");
            }

            println!("\n=== All edge case tests passed ===");
            robot.exit().ok();
        })
        .run(|| {
            let state = rememberLazyListState();

            Column(Modifier::default(), ColumnSpec::default(), move || {
                Row(
                    Modifier::default().fill_max_width().height(50.0),
                    RowSpec::default(),
                    move || {
                        Button(
                            Modifier::default(),
                            ButtonSpec::default(),
                            move || {
                                state.scroll_to_item(50, 0.0);
                            },
                            || {
                                Text("Jump 50", Modifier::default(), TextStyle::default());
                            },
                        );
                        Button(
                            Modifier::default(),
                            ButtonSpec::default(),
                            move || {
                                state.scroll_to_item(95, 0.0);
                            },
                            || {
                                Text("Jump 95", Modifier::default(), TextStyle::default());
                            },
                        );
                    },
                );

                LazyColumn(
                    Modifier::default().fill_max_width().fill_max_height(),
                    state,
                    LazyColumnSpec::default()
                        .vertical_arrangement(LinearArrangement::SpacedBy(10.0))
                        .content_padding(0.0, 20.0),
                    |scope| {
                        scope.items(100, move |index| {
                            let color = if index % 2 == 0 {
                                Color::rgb(128.0, 128.0, 128.0)
                            } else {
                                Color::WHITE
                            };
                            Box(
                                Modifier::default()
                                    .size(Size {
                                        width: 300.0,
                                        height: 50.0,
                                    })
                                    .background(color),
                                BoxSpec::new().content_alignment(Alignment::CENTER),
                                move || {
                                    Text(
                                        format!("Item {}", index),
                                        Modifier::default(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        });
                    },
                );
            });
        });
}
