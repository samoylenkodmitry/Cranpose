mod robot_launch;

use std::time::Duration;

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

    robot_launch::launch("Lazy Complex Scroll Test", 400, 800)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));

            println!("--- Phase 1: Initial Layout ---");
            let (_, y, _, _) = find_text_in_semantics(&robot, "Item 0").expect("Item 0 missing");
            println!("Item 0: y={:.1}", y);
            assert!((y - 65.2).abs() < 5.0, "Item 0 should be at ~65.2");

            let (_, y, _, _) = find_text_in_semantics(&robot, "Item 1").expect("Item 1 missing");
            println!("Item 1: y={:.1}", y);
            assert!((y - 140.2).abs() < 5.0, "Item 1 should be at ~140.2");

            println!("--- Phase 2: Jump to 50 ---");
            let (bx, by, bw, bh) =
                find_button_in_semantics(&robot, "Jump 50").expect("Jump button missing");
            robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
            std::thread::sleep(Duration::from_millis(500));

            let (_, y, _, _) = find_text_in_semantics(&robot, "Item 50")
                .expect("Item 50 should be visible after jump");
            println!("Item 50 found at y={:.1}", y);

            if find_text_in_semantics(&robot, "Item 0").is_some() {
                panic!("Item 0 should be recycled/virtualized out!");
            }

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
                    },
                );

                LazyColumn(
                    Modifier::default().fill_max_width().fill_max_height(),
                    state,
                    LazyColumnSpec::default(),
                    |scope| {
                        scope.items(100, move |index| {
                            let height = match index % 3 {
                                0 => 50.0,
                                1 => 100.0,
                                _ => 150.0,
                            };
                            let color = match index % 3 {
                                0 => Color::RED,
                                1 => Color::GREEN,
                                _ => Color::BLUE,
                            };

                            Box(
                                Modifier::default()
                                    .size(Size {
                                        width: 300.0,
                                        height,
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
