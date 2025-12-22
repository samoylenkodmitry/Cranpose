use compose_app::AppLauncher;
use compose_foundation::lazy::{LazyListIntervalContent, LazyListState, LazyListScope};
use compose_ui::widgets::{Box, BoxSpec, Button, Text, Column, ColumnSpec, Row, RowSpec};
use compose_ui::{Modifier, Size, Color, Alignment};
use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
use compose_testing::{find_text_in_semantics, find_button_in_semantics};
use std::time::Duration;

fn main() {
    env_logger::init();
    
    AppLauncher::new()
        .with_title("Lazy Infinite Scroll Test")
        .with_size(400, 600) // Viewport height 600
        .with_test_driver(|robot| {
             std::thread::sleep(Duration::from_millis(500)); 
             
             println!("--- Phase 1: Initial Layout ---");
             // 0..7 should be visible (roughly)
             // Heights:
             // 0: 0
             // 1: 48
             // 2: 96
             // 3: 144
             // 4: 192 (Total 480 used)
             // 5: 0
             // 6: 48 
             // 7: 96 (Total 624 used)
             // So Item 7 might be partially visible.
             
             if find_text_in_semantics(&robot, "Item 0").is_none() {
                 println!("Item 0 (Height 0) might be invisible/layout-out? Text should be there if content emitted.");
             }
             // Actually, if height is 0, Box is 0 height. Text inside might be clipped?
             // Or Text size enforces min size?
             // Box modifier size(w, 0).
             // If Box is 0 height, and valid constraints are 0..0, Text measurables might effectively be 0 or clipped.
             // If Text measures to 19.6, but MaxHeight is 0.
             // It depends on Box implementation.
             // If Item 0 layout is 0 size, testing finding "Item 0" might be flaky if semantics skips empty nodes.
             // Let's check Item 1.
             
             let (_, y1, _, _) = find_text_in_semantics(&robot, "Item 1").expect("Item 1 missing");
             println!("Item 1 y={:.1}", y1);
             // Item 0 is 0 height. Item 1 starts at 0?
             // Or Item 0 has 50px header above it?
             // Header Row 50px.
             // Item 0 (0px). Item 1 (48px).
             // Item 1 starts at 50 + 0 = 50.
             // Text centered in 48: (48-19.6)/2 = 14.2.
             // Global y = 50 + 14.2 = 64.2.
             assert!((y1 - 64.2).abs() < 5.0, "Item 1 at y={:.1}, expected ~64.2", y1);

             println!("--- Phase 2: Jump to 99,990 ---");
             let (bx, by, bw, bh) = find_button_in_semantics(&robot, "Jump 1M").expect("Jump button missing");
             robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
             std::thread::sleep(Duration::from_millis(500));

             // Target Index: 99_990
             // 99990 % 5 = 0. Height 0.
             // Item 99991 Height 48.
             
             if find_text_in_semantics(&robot, "Item 99990").is_none() {
                 println!("Item 99990 (0 height) likely skipped/invisible.");
             }
             
             let (_, y_next, _, _) = find_text_in_semantics(&robot, "Item 99991").expect("Item 99991 missing");
             println!("Item 99991 y={:.1}", y_next);
             
             assert!((y_next - 64.2).abs() < 5.0, "Item 99991 at y={:.1}, expected ~64.2", y_next);
             
             // Ensure Item 1 is gone
             if find_text_in_semantics(&robot, "Item 1").is_some() {
                 panic!("Item 1 should be gone!");
             }

             robot.exit().ok();
        })
        .run(move || {
            let state = LazyListState::new();
            let state_clone = state.clone();

            Column(Modifier::default(), ColumnSpec::default(), move || {
                // Controls
                let row_state = state_clone.clone();
                
                Row(Modifier::default().fill_max_width().height(50.0), RowSpec::default(), move || {
                    let state = row_state.clone();
                    Button(
                        Modifier::default(), 
                        move || {
                            state.clone().scroll_to_item(99_990, 0.0);
                        }, 
                        || { Text("Jump 1M", Modifier::default()); }
                    );
                });

                let state_clone_2 = state.clone();

                Box(
                    Modifier::default().fill_max_width().height(550.0),
                    BoxSpec::default(),
                    move || {
                        // List content defined inside closure to avoid move errors
                        let items_content = {
                            let mut content = LazyListIntervalContent::new();
                            // Debugging count: 100k
                            let count = 100_000; 
                            content.items(count, None::<fn(usize)->u64>, None::<fn(usize)->u64>, move |index| {
                                let height = 48.0 * (index % 5) as f32;
                                
                                // Color cycle
                                let color = match index % 5 {
                                    0 => Color::rgb(1.0, 0.0, 0.0), // Red
                                    1 => Color::rgb(0.0, 1.0, 0.0), // Green
                                    2 => Color::rgb(0.0, 0.0, 1.0), // Blue
                                    3 => Color::rgb(1.0, 1.0, 0.0), // Yellow
                                    _ => Color::rgb(0.0, 1.0, 1.0), // Cyan
                                };
                                
                                Box(
                                    Modifier::default()
                                        .size(Size { width: 300.0, height })
                                        .background(color),
                                    BoxSpec::new().content_alignment(Alignment::CENTER),
                                    move || { 
                                        Text(format!("Item {}", index), Modifier::default()); 
                                    }
                                );
                            });
                            content
                        };

                        LazyColumn(
                            Modifier::default().fill_max_width().fill_max_height(), 
                            state_clone_2.clone(),
                            LazyColumnSpec::default(),
                            items_content,
                        );
                    }
                );
            });
        });
}
