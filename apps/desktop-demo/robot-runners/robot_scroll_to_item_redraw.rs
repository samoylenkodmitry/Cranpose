use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::{find_button_in_semantics, find_text_in_semantics};
use cranpose_ui::{
    widgets::{
        Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row,
        RowSpec, Text,
    },
    Alignment, Color, Modifier, Size, TextStyle,
};

fn main() {
    env_logger::init();

    AppLauncher::new()
        .with_title("scroll_to_item Redraw Test")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(300));

            println!("=== Phase 1: Initial state - Item 0 should be visible ===");
            match find_text_in_semantics(&robot, "Item 0") {
                Some((x, y, w, h)) => {
                    println!("✓ Item 0 found at ({:.1}, {:.1}, {:.1}x{:.1})", x, y, w, h);
                }
                None => {
                    panic!("FAIL: Item 0 not visible at startup!");
                }
            }

            if find_text_in_semantics(&robot, "Item 50").is_some() {
                panic!("FAIL: Item 50 should NOT be visible at startup!");
            }
            println!("✓ Item 50 correctly not visible at startup");

            println!("\n=== Phase 2: Click 'Jump to 50' button ===");
            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Jump to 50").expect("Jump to 50 button missing");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();

            std::thread::sleep(Duration::from_millis(200));

            println!("\n=== Phase 3: Item 50 MUST be visible immediately after button click ===");
            match find_text_in_semantics(&robot, "Item 50") {
                Some((x, y, w, h)) => {
                    println!("✓ Item 50 found at ({:.1}, {:.1}, {:.1}x{:.1})", x, y, w, h);
                    println!("✓ PASS: scroll_to_item triggered immediate redraw!");
                }
                None => {
                    println!();
                    println!("╔════════════════════════════════════════════════════════════════╗");
                    println!("║  FAIL: Item 50 NOT visible after scroll_to_item!               ║");
                    println!("║                                                                ║");
                    println!("║  This indicates the redraw bug is present:                     ║");
                    println!("║  scroll_to_item updated data but didn't trigger render.        ║");
                    println!("╚════════════════════════════════════════════════════════════════╝");
                    println!();
                    panic!("REDRAW BUG: Item 50 NOT visible after scroll_to_item!");
                }
            }

            if find_text_in_semantics(&robot, "Item 0").is_some() {
                println!("WARNING: Item 0 still visible - may indicate incomplete scroll");
            } else {
                println!("✓ Item 0 correctly scrolled out");
            }

            println!("\n=== All Tests Passed ===");
            robot.exit().ok();
        })
        .run(|| {
            let state = rememberLazyListState();

            Column(
                Modifier::default().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    Row(
                        Modifier::default().fill_max_width().height(50.0),
                        RowSpec::default(),
                        move || {
                            Button(
                                Modifier::default().background(Color::rgb(0.3, 0.4, 0.6)),
                                ButtonSpec::default(),
                                move || {
                                    state.scroll_to_item(50, 0.0);
                                },
                                || {
                                    Text("Jump to 50", Modifier::default(), TextStyle::default());
                                },
                            );
                        },
                    );

                    Box(
                        Modifier::default().fill_max_width().weight(1.0),
                        BoxSpec::new().content_alignment(Alignment::TOP_START),
                        move || {
                            LazyColumn(
                                Modifier::default().fill_max_size(),
                                state,
                                LazyColumnSpec::default(),
                                |scope| {
                                    scope.items(100, move |index| {
                                        let bg = if index % 2 == 0 {
                                            Color::rgb(0.2, 0.3, 0.4)
                                        } else {
                                            Color::rgb(0.3, 0.4, 0.5)
                                        };

                                        Box(
                                            Modifier::default()
                                                .size(Size {
                                                    width: 300.0,
                                                    height: 50.0,
                                                })
                                                .background(bg),
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
                        },
                    );
                },
            );
        });
}
