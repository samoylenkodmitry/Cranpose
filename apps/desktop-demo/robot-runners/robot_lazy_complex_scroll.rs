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
        .with_title("Lazy Complex Scroll Test")
        .with_size(400, 800)
        .with_test_driver(|robot| {
             std::thread::sleep(Duration::from_millis(500)); 
             
             println!("--- Phase 1: Initial Layout ---");
             // Check Item 0 (Height 50) at 0
             let (x, y, w, h) = find_text_in_semantics(&robot, "Item 0").expect("Item 0 missing");
             println!("Item 0: y={:.1}", y);
             // Text centered in 50px. 50-19.6 = 30.4 / 2 = 15.2. + 50px header = 65.2
             assert!((y - 65.2).abs() < 5.0, "Item 0 should be at ~65.2");

             // Check Item 1 (Height 100) at 50 + 50 = 100
             let (x, y, w, h) = find_text_in_semantics(&robot, "Item 1").expect("Item 1 missing");
             println!("Item 1: y={:.1}", y);
             // Starts at 100. Height 100. Center = 100 + 40.2 = 140.2.
             assert!((y - 140.2).abs() < 5.0, "Item 1 should be at ~140.2");

             println!("--- Phase 2: Jump to 50 ---");
             let (bx, by, bw, bh) = find_button_in_semantics(&robot, "Jump 50").expect("Jump button missing");
             robot.click(bx + bw / 2.0, by + bh / 2.0).ok();
             std::thread::sleep(Duration::from_millis(500)); // Wait for scroll/layout

             // Verify Item 50 is visible
             // Item heights cycle: 50, 100, 150.
             // Item 50 is index 50. 50 % 3 = 2. Height = 150.
             // Should be at top of list (y=0 relative to list).
             // List is below buttons. Buttons ~40px high?
             // We'll check it's visible.
             
             let (x, y, w, h) = find_text_in_semantics(&robot, "Item 50").expect("Item 50 should be visible after jump");
             println!("Item 50 found at y={:.1}", y);
             
             // Check that Item 0 is GONE (virtualized out)
             if find_text_in_semantics(&robot, "Item 0").is_some() {
                 panic!("Item 0 should be recycled/virtualized out!");
             }

             // Verify position: Item 50 should be near the top of the LazyColumn.
             // LazyColumn is in a Column, below Row of buttons.
             // If Row is height ~50, then Item 50 should be around y=50 + centering offset.
             // Item 50 height = 150. Center offset = (150-19.6)/2 = 65.2.
             // Expected y ~ 50 + 65.2 = 115.2.
             // Let's print and see.
             
             robot.exit().ok();
        })
        .run(|| {
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
                            // On Click
                            state.clone().scroll_to_item(50, 0.0);
                        }, 
                        || { Text("Jump 50", Modifier::default()); }
                    );
                });

                // List
                let items_content = {
                    let mut content = LazyListIntervalContent::new();
                    content.items(100, None::<fn(usize)->u64>, None::<fn(usize)->u64>, move |index| {
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
                    state.clone(),
                    LazyColumnSpec::default(),
                    items_content,
                );
            });
        });
}
