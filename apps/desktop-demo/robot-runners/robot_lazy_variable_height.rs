use compose_app::AppLauncher;
use compose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use compose_testing::find_text_in_semantics;
use compose_ui::widgets::{Box, BoxSpec};
use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
use compose_ui::{Alignment, Color, Modifier, Size};
use std::time::Duration;

fn main() {
    env_logger::init();

    AppLauncher::new()
        .with_title("Lazy Variable Height Test")
        .with_size(400, 600)
        .with_test_driver(|robot| {
             // Step 1: Verify Initial Layout Offsets
             std::thread::sleep(Duration::from_millis(500)); // Wait for first frame

             let check_item = |name: &str, expected_y: f32, _expected_height: f32| {
                 if let Some((_, y, _, h)) = find_text_in_semantics(&robot, name) {
                     println!("Found {} at y={:.1}, h={:.1}", name, y, h);

                     // Check position
                     let y_diff = (y - expected_y).abs();
                     if y_diff > 1.0 {
                         println!("  BUG: {} y={:.1} but expected {:.1}", name, y, expected_y);
                     }
                 } else {
                     panic!("{} not found", name);
                 }
             };

             // Item 0: h=100. Text centered -> ~40.2
             check_item("Item0", 40.2, 19.6);

             // Item 1: starts at 100. h=50. Text centered -> 100 + (50-19.6)/2 = 115.2
             if let Some((_, y, _, _)) = find_text_in_semantics(&robot, "Item1") {
                 if y < 99.0 {
                    panic!("CONFIRMED BUG: Item1 is at y={:.1}, expected > 100.0. Measuring is broken!", y);
                 }
                 let expected = 115.2;
                 if (y - expected).abs() > 2.0 {
                     println!("Warning: Item1 y={:.1}, expected {:.1}", y, expected);
                 }
             }

             robot.exit().ok();
        })
        .run(|| {
            // Define the UI within the app
            let state = remember_lazy_list_state();

            LazyColumn(
                Modifier::default().size(Size { width: 300.0, height: 400.0 }),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    // Item 0: Height 100
                    scope.item(Some(0), None, move || {
                        Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 100.0 })
                                .background(Color::RED),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { compose_ui::widgets::Text("Item0", Modifier::default()); }
                        );
                    });

                    // Item 1: Height 50
                    scope.item(Some(1), None, move || {
                        Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 50.0 })
                                .background(Color::GREEN),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { compose_ui::widgets::Text("Item1", Modifier::default()); }
                        );
                    });

                    // Item 2: Height 200
                    scope.item(Some(2), None, move || {
                         Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 200.0 })
                                .background(Color::BLUE),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { compose_ui::widgets::Text("Item2", Modifier::default()); }
                        );
                    });
                },
            );
        });
}
