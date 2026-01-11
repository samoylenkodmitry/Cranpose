//! Robot test for verifying scroll_to_item triggers immediate redraw.
//!
//! This test validates that programmatic scroll (scroll_to_item) causes
//! the UI to update immediately, without requiring user interaction.
//!
//! BUG: Before fix, scroll_to_item would update data but not request a render,
//! so the UI wouldn't update until the next user scroll/click.

use compose_app::AppLauncher;
use compose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use compose_testing::{find_button_in_semantics, find_text_in_semantics};
use compose_ui::widgets::{
    Box, BoxSpec, Button, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row, RowSpec, Text,
};
use compose_ui::{Alignment, Color, Modifier, Size};
use std::time::Duration;

fn main() {
    env_logger::init();

    AppLauncher::new()
        .with_title("scroll_to_item Redraw Test")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(300));

            // Phase 1: Verify initial state - Item 0 should be visible
            println!("=== Phase 1: Initial state - Item 0 should be visible ===");
            match find_text_in_semantics(&robot, "Item 0") {
                Some((x, y, w, h)) => {
                    println!("✓ Item 0 found at ({:.1}, {:.1}, {:.1}x{:.1})", x, y, w, h);
                }
                None => {
                    panic!("FAIL: Item 0 not visible at startup!");
                }
            }

            // Verify Item 50 is NOT visible initially (we only have 100 items)
            if find_text_in_semantics(&robot, "Item 50").is_some() {
                panic!("FAIL: Item 50 should NOT be visible at startup!");
            }
            println!("✓ Item 50 correctly not visible at startup");

            // Phase 2: Click "Jump to 50" button
            println!("\n=== Phase 2: Click 'Jump to 50' button ===");
            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Jump to 50").expect("Jump to 50 button missing");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();

            // Wait a bit for the frame to render (but NOT for user interaction)
            std::thread::sleep(Duration::from_millis(200));

            // Phase 3: Verify Item 50 is NOW visible (this is the critical test!)
            // If the redraw bug exists, Item 50 would NOT be visible yet.
            println!("\n=== Phase 3: Item 50 MUST be visible immediately after button click ===");
            match find_text_in_semantics(&robot, "Item 50") {
                Some((x, y, w, h)) => {
                    println!("✓ Item 50 found at ({:.1}, {:.1}, {:.1}x{:.1})", x, y, w, h);
                    println!("✓ PASS: scroll_to_item triggered immediate redraw!");
                }
                None => {
                    println!("");
                    println!("╔════════════════════════════════════════════════════════════════╗");
                    println!("║  FAIL: Item 50 NOT visible after scroll_to_item!               ║");
                    println!("║                                                                ║");
                    println!("║  This indicates the redraw bug is present:                     ║");
                    println!("║  scroll_to_item updated data but didn't trigger render.        ║");
                    println!("╚════════════════════════════════════════════════════════════════╝");
                    println!("");
                    panic!("REDRAW BUG: Item 50 NOT visible after scroll_to_item!");
                }
            }

            // Verify Item 0 is NO LONGER visible (scrolled out)
            if find_text_in_semantics(&robot, "Item 0").is_some() {
                println!("WARNING: Item 0 still visible - may indicate incomplete scroll");
            } else {
                println!("✓ Item 0 correctly scrolled out");
            }

            println!("\n=== All Tests Passed ===");
            robot.exit().ok();
        })
        .run(|| {
            let state = remember_lazy_list_state();

            Column(
                Modifier::default().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    // Control button
                    Row(
                        Modifier::default().fill_max_width().height(50.0),
                        RowSpec::default(),
                        move || {
                            Button(
                                Modifier::default().background(Color::rgb(0.3, 0.4, 0.6)),
                                move || {
                                    // This should immediately show Item 50 at the top
                                    state.scroll_to_item(50, 0.0);
                                },
                                || {
                                    Text("Jump to 50", Modifier::default());
                                },
                            );
                        },
                    );

                    // LazyColumn with 100 items
                    Box(
                        Modifier::default().fill_max_width().weight(1.0),
                        BoxSpec::new().content_alignment(Alignment::TOP_START),
                        move || {
                            LazyColumn(
                                Modifier::default().fill_max_size(),
                                state,
                                LazyColumnSpec::default(),
                                |scope| {
                                    scope.items(
                                        100,
                                        None::<fn(usize) -> u64>,
                                        None::<fn(usize) -> u64>,
                                        move |index| {
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
                                                    );
                                                },
                                            );
                                        },
                                    );
                                },
                            );
                        },
                    );
                },
            );
        });
}
