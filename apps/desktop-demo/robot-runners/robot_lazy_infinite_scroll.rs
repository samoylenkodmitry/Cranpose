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
        .with_title("Lazy Infinite Scroll Test")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
             std::thread::sleep(Duration::from_millis(500));

             println!("--- Phase 1: Initial Layout ---");

             if find_text_in_semantics(&robot, "Item 0").is_none() {
                 println!("Item 0 (Height 0) might be invisible/layout-out? Text should be there if content emitted.");
             }

             let (_, y1, _, _) = find_text_in_semantics(&robot, "Item 1").expect("Item 1 missing");
             println!("Item 1 y={:.1}", y1);
             assert!((y1 - 64.2).abs() < 5.0, "Item 1 at y={:.1}, expected ~64.2", y1);

             println!("--- Phase 2: Jump to 99,990 ---");
             let (bx, by, bw, bh) = find_button_in_semantics(&robot, "Jump 1M").expect("Jump button missing");
             robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
             std::thread::sleep(Duration::from_millis(500));


             if find_text_in_semantics(&robot, "Item 99990").is_none() {
                 println!("Item 99990 (0 height) likely skipped/invisible.");
             }

             let (_, y_next, _, _) = find_text_in_semantics(&robot, "Item 99991").expect("Item 99991 missing");
             println!("Item 99991 y={:.1}", y_next);

             assert!((y_next - 64.2).abs() < 5.0, "Item 99991 at y={:.1}, expected ~64.2", y_next);

             if find_text_in_semantics(&robot, "Item 1").is_some() {
                 panic!("Item 1 should be gone!");
             }

             robot.exit().ok();
        })
        .run(move || {
            let state = rememberLazyListState();

            Column(Modifier::default(), ColumnSpec::default(), move || {
                Row(Modifier::default().fill_max_width().height(50.0), RowSpec::default(), move || {
                    Button(
                        Modifier::default(), ButtonSpec::default(),
                        move || {
                            state.scroll_to_item(99_990, 0.0);
                        },
                        || { Text("Jump 1M", Modifier::default(), TextStyle::default()); }
                    );
                });

                Box(
                    Modifier::default().fill_max_width().height(550.0),
                    BoxSpec::default(),
                    move || {
                        let count = 100_000;
                        LazyColumn(
                            Modifier::default().fill_max_width().fill_max_height(),
                            state,
                            LazyColumnSpec::default(),
                            |scope| {
                                scope.items(count, move |index| {
                                    let height = 48.0 * (index % 5) as f32;

                                    let color = match index % 5 {
                                        0 => Color::rgb(1.0, 0.0, 0.0),
                                        1 => Color::rgb(0.0, 1.0, 0.0),
                                        2 => Color::rgb(0.0, 0.0, 1.0),
                                        3 => Color::rgb(1.0, 1.0, 0.0),
                                        _ => Color::rgb(0.0, 1.0, 1.0),
                                    };

                                    Box(
                                        Modifier::default()
                                            .size(Size { width: 300.0, height })
                                            .background(color),
                                        BoxSpec::new().content_alignment(Alignment::CENTER),
                                        move || {
                                            Text(format!("Item {}", index), Modifier::default(), TextStyle::default());
                                        }
                                    );
                                });
                            },
                        );
                    }
                );
            });
        });
}
