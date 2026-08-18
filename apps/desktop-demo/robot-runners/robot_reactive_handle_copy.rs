use cranpose::AppLauncher;
use cranpose_core::remember;
use cranpose_ui::{
    composable, rememberMutableInteractionSource, Box as ComposeBox, BoxSpec, Column, ColumnSpec,
    Modifier, Row, RowSpec, ScrollState, Text, TextStyle,
};

#[composable]
fn reactive_handle_copy_screen() {
    let scroll_state = remember(|| ScrollState::new(0.0)).with(|state| state.clone());
    let interaction_source = rememberMutableInteractionSource();
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Row(Modifier::empty(), RowSpec::default(), move || {
            ComposeBox(
                Modifier::empty()
                    .horizontal_scroll(scroll_state, false)
                    .press_interaction_source(interaction_source),
                BoxSpec::default(),
                move || {
                    Text("left", Modifier::empty(), TextStyle::default());
                },
            );
            ComposeBox(
                Modifier::empty()
                    .horizontal_scroll(scroll_state, false)
                    .press_interaction_source(interaction_source),
                BoxSpec::default(),
                move || {
                    Text("right", Modifier::empty(), TextStyle::default());
                },
            );
        });
    });
}

fn main() {
    AppLauncher::new()
        .with_title("Reactive Handle Copy")
        .with_size(640, 240)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            robot.validate_content("left").expect("left content");
            robot.validate_content("right").expect("right content");
            robot.exit().expect("robot exit");
        })
        .run(reactive_handle_copy_screen);
}
