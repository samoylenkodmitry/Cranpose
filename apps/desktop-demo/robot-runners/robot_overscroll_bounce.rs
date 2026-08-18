use cranpose::AppLauncher;
use cranpose_core::remember;
use cranpose_testing::find_text_in_semantics;
use cranpose_ui::widgets::{Column, ColumnSpec, Spacer, Text};
use cranpose_ui::{composable, Modifier, ScrollState, Size, TextStyle};
use std::time::Duration;

#[composable]
fn overscroll_reproduction() {
    let state = remember(|| ScrollState::new(0.0)).with(|state| state.clone());
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(state, false),
        ColumnSpec::default(),
        || {
            Text("Overscroll Marker", Modifier::empty(), TextStyle::default());
            Spacer(Size {
                width: 0.0,
                height: 1_200.0,
            });
        },
    );
}

fn main() {
    AppLauncher::new()
        .with_title("Overscroll Bounce Reproduction")
        .with_size(400, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let (_, initial_y, width, height) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            let x = width * 0.5;
            let y = initial_y + height * 0.5;
            robot.mouse_move(x, y).expect("move to marker");
            robot.mouse_down().expect("press marker");
            robot.mouse_move(x, y + 140.0).expect("drag past top edge");
            std::thread::sleep(Duration::from_millis(100));

            let (_, stretched_y, _, _) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            assert!(
                stretched_y > initial_y + 20.0,
                "top-edge drag must stretch content, initial_y={initial_y}, stretched_y={stretched_y}"
            );

            robot.mouse_up().expect("release marker");
            std::thread::sleep(Duration::from_millis(700));
            let (_, settled_y, _, _) =
                find_text_in_semantics(&robot, "Overscroll Marker").expect("marker missing");
            assert!(
                (settled_y - initial_y).abs() < 4.0,
                "overscroll must spring back to the edge, initial_y={initial_y}, settled_y={settled_y}"
            );
            robot.exit().expect("exit");
        })
        .run(overscroll_reproduction);
}
