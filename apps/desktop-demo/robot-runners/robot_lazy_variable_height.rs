use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::{
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
    Alignment, Color, Modifier, Size, TextStyle,
};

fn main() {
    env_logger::init();

    AppLauncher::new()
        .with_title("Lazy Variable Height Test")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
             std::thread::sleep(Duration::from_millis(500));

             let check_item = |name: &str, expected_y: f32, _expected_height: f32| {
                 if let Some((_, y, _, h)) = find_text_in_semantics(&robot, name) {
                     println!("Found {} at y={:.1}, h={:.1}", name, y, h);

                     let y_diff = (y - expected_y).abs();
                     if y_diff > 1.0 {
                         println!("  BUG: {} y={:.1} but expected {:.1}", name, y, expected_y);
                     }
                 } else {
                     panic!("{} not found", name);
                 }
             };

             check_item("Item0", 40.2, 19.6);

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
            let state = rememberLazyListState();

            LazyColumn(
                Modifier::default().size(Size { width: 300.0, height: 400.0 }),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.item_keyed(Some(0), None, move || {
                        Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 100.0 })
                                .background(Color::RED),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { cranpose_ui::widgets::Text("Item0", Modifier::default(), TextStyle::default()); }
                        );
                    });

                    scope.item_keyed(Some(1), None, move || {
                        Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 50.0 })
                                .background(Color::GREEN),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { cranpose_ui::widgets::Text("Item1", Modifier::default(), TextStyle::default()); }
                        );
                    });

                    scope.item_keyed(Some(2), None, move || {
                         Box(
                            Modifier::default()
                                .size(Size { width: 100.0, height: 200.0 })
                                .background(Color::BLUE),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            || { cranpose_ui::widgets::Text("Item2", Modifier::default(), TextStyle::default()); }
                        );
                    });
                },
            );
        });
}
