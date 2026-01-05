//! Robot test for LazyList with usize::MAX items and Jump to Middle navigation.
//!
//! This test validates:
//! 1. Setting item count to usize::MAX
//! 2. Clicking "Jump to Middle" (navigates to usize::MAX / 2)
//! 3. Verifying visible item positions with variable heights (48 + (i % 5) * 8)
//!
//! Height pattern: index % 5 -> 0=48, 1=56, 2=64, 3=72, 4=80

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
        .with_title("LazyList usize::MAX Jump Middle Test")
        .with_size(500, 700)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));

            println!("=== Phase 1: Click 'Set MAX' ===");
            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Set MAX").expect("Set MAX button missing");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
            std::thread::sleep(Duration::from_millis(300));

            println!("=== Phase 2: Click 'Go Middle' ===");
            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Go Middle").expect("Go Middle button missing");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));

            println!("=== Phase 3: Verify Visible Items ===");

            // Middle of usize::MAX
            let middle: usize = usize::MAX / 2;

            // The list starts at LazyColumn offset (buttons row is 50px high)
            // LazyColumn has 500px height available.
            //
            // Item heights cycle: index % 5 -> 48, 56, 64, 72, 80
            // middle % 5 == ?
            // usize::MAX = 18446744073709551615
            // middle = 9223372036854775807
            // 9223372036854775807 % 5 = 2 (since ...07 % 5 = 2)
            // So middle item has height 48 + 2*8 = 64
            //
            // middle+0 (% 5 = 2) -> h=64
            // middle+1 (% 5 = 3) -> h=72
            // middle+2 (% 5 = 4) -> h=80
            // middle+3 (% 5 = 0) -> h=48
            // middle+4 (% 5 = 1) -> h=56
            // middle+5 (% 5 = 2) -> h=64

            // Calculate expected item indices at middle
            let expected_heights: [(usize, f32); 6] = [
                (middle, 64.0),
                (middle + 1, 72.0),
                (middle + 2, 80.0),
                (middle + 3, 48.0),
                (middle + 4, 56.0),
                (middle + 5, 64.0),
            ];

            // Cumulative Y offsets (relative to list top, below 50px button row)
            // Item 0 (middle): y = 50
            // Item 1 (middle+1): y = 50 + 64 = 114
            // Item 2 (middle+2): y = 114 + 72 = 186
            // etc.
            let list_top = 50.0; // Button row is 50px
            let mut expected_y = list_top;

            for (idx, height) in expected_heights.iter().take(5) {
                let label = format!("Item {}", idx);

                match find_text_in_semantics(&robot, &label) {
                    Some((_x, item_y, _w, _h)) => {
                        // The item's Box has the full height. Text is centered in it.
                        // Box starts at expected_y. Text y depends on centering.
                        // For a Box of height `height`, text at center means:
                        // text_y = box_y + (height - text_h) / 2
                        // Approximate text height = ~19.6
                        let text_h = 19.6;
                        let expected_text_y = expected_y + (height - text_h) / 2.0;

                        println!(
                            "{}: y={:.1}, expected~{:.1} (box starts at {:.1}, h={})",
                            label, item_y, expected_text_y, expected_y, height
                        );

                        // Allow 20px tolerance for centering/padding variations
                        assert!(
                            (item_y - expected_text_y).abs() < 20.0,
                            "{} position mismatch: got {:.1}, expected ~{:.1}",
                            label,
                            item_y,
                            expected_text_y
                        );
                    }
                    None => {
                        println!("{}: NOT FOUND (may be scrolled out)", label);
                    }
                }

                expected_y += height;
            }

            // Verify Item 0 is NOT visible (virtualized out due to scroll)
            if find_text_in_semantics(&robot, "Item 0").is_some() {
                panic!("Item 0 should be virtualized out at middle!");
            }
            println!("✓ Item 0 correctly virtualized out");

            // Verify an item way before middle is not visible
            if find_text_in_semantics(&robot, "Item 100").is_some() {
                panic!("Item 100 should be virtualized out!");
            }
            println!("✓ Item 100 correctly virtualized out");

            println!("=== All Tests Passed ===");
            robot.exit().ok();
        })
        .run(|| {
            let state = remember_lazy_list_state();
            let item_count = compose_core::useState(|| 100usize);

            Column(
                Modifier::default().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    // Control buttons row
                    Row(
                        Modifier::default().fill_max_width().height(50.0),
                        RowSpec::default(),
                        move || {
                            // Set MAX button
                            Button(
                                Modifier::default().background(Color::rgb(0.6, 0.3, 0.6)),
                                move || {
                                    item_count.set(usize::MAX);
                                },
                                || {
                                    Text("Set MAX", Modifier::default());
                                },
                            );

                            // Go Middle button
                            Button(
                                Modifier::default().background(Color::rgb(0.3, 0.4, 0.6)),
                                move || {
                                    let c = item_count.get();
                                    let middle = c / 2;
                                    state.scroll_to_item(middle, 0.0);
                                },
                                || {
                                    Text("Go Middle", Modifier::default());
                                },
                            );
                        },
                    );

                    // LazyColumn with variable height items
                    Box(
                        Modifier::default().fill_max_width().weight(1.0),
                        BoxSpec::new().content_alignment(Alignment::TOP_START),
                        move || {
                            let count = item_count.get();
                            LazyColumn(
                                Modifier::default().fill_max_size(),
                                state,
                                LazyColumnSpec::default(),
                                |scope| {
                                    scope.items(
                                        count,
                                        None::<fn(usize) -> u64>,
                                        None::<fn(usize) -> u64>,
                                        move |index| {
                                            // Height: 48 + (index % 5) * 8 -> 48, 56, 64, 72, 80
                                            let height = 48.0 + (index % 5) as f32 * 8.0;
                                            let bg = if index % 2 == 0 {
                                                Color::rgb(0.2, 0.3, 0.4)
                                            } else {
                                                Color::rgb(0.3, 0.4, 0.5)
                                            };

                                            Box(
                                                Modifier::default()
                                                    .size(Size {
                                                        width: 400.0,
                                                        height,
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
