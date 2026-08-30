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
        .with_title("LazyList usize::MAX Jump Middle Test")
        .with_size(500, 700)
        .with_headless(true)
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

            let middle: usize = usize::MAX / 2;

            let expected_heights: [(usize, f32); 6] = [
                (middle, 64.0),
                (middle + 1, 72.0),
                (middle + 2, 80.0),
                (middle + 3, 48.0),
                (middle + 4, 56.0),
                (middle + 5, 64.0),
            ];

            let list_top = 50.0;
            let mut expected_y = list_top;

            for (idx, height) in expected_heights.iter().take(5) {
                let label = format!("Item {}", idx);

                match find_text_in_semantics(&robot, &label) {
                    Some((_x, item_y, _w, _h)) => {
                        let text_h = 19.6;
                        let expected_text_y = expected_y + (height - text_h) / 2.0;

                        println!(
                            "{}: y={:.1}, expected~{:.1} (box starts at {:.1}, h={})",
                            label, item_y, expected_text_y, expected_y, height
                        );

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

            if find_text_in_semantics(&robot, "Item 0").is_some() {
                panic!("Item 0 should be virtualized out at middle!");
            }
            println!("✓ Item 0 correctly virtualized out");

            if find_text_in_semantics(&robot, "Item 100").is_some() {
                panic!("Item 100 should be virtualized out!");
            }
            println!("✓ Item 100 correctly virtualized out");

            println!("=== All Tests Passed ===");
            robot.exit().ok();
        })
        .run(|| {
            let state = rememberLazyListState();
            let item_count = cranpose_core::rememberMutableStateOf(|| 100usize);

            Column(
                Modifier::default().fill_max_size(),
                ColumnSpec::default(),
                {
                    move || {
                        Row(
                            Modifier::default().fill_max_width().height(50.0),
                            RowSpec::default(),
                            {
                                move || {
                                    Button(
                                        Modifier::default().background(Color::rgb(0.6, 0.3, 0.6)),
                                        ButtonSpec::default(),
                                        move || {
                                            item_count.set(usize::MAX);
                                        },
                                        || {
                                            Text(
                                                "Set MAX",
                                                Modifier::default(),
                                                TextStyle::default(),
                                            );
                                        },
                                    );

                                    Button(
                                        Modifier::default().background(Color::rgb(0.3, 0.4, 0.6)),
                                        ButtonSpec::default(),
                                        move || {
                                            let c = item_count.get();
                                            let middle = c / 2;
                                            state.scroll_to_item(middle, 0.0);
                                        },
                                        || {
                                            Text(
                                                "Go Middle",
                                                Modifier::default(),
                                                TextStyle::default(),
                                            );
                                        },
                                    );
                                }
                            },
                        );

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
                                        scope.items(count, move |index| {
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
                                                        TextStyle::default(),
                                                    );
                                                },
                                            );
                                        });
                                    },
                                );
                            },
                        );
                    }
                },
            );
        });
}
