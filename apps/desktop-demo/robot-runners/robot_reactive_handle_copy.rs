mod robot_launch;

use cranpose_core::remember;
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable, rememberMutableInteractionSource, round_scaling_list::CentreAnchor,
    widgets::wear::rememberWearScalingListState, Box as ComposeBox, BoxSpec, Column, ColumnSpec,
    Modifier, Row, RowSpec, ScrollState, Text, TextStyle, ZoomState,
};

#[composable]
fn reactive_handle_copy_screen() {
    let scroll_state = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let interaction_source = rememberMutableInteractionSource();
    let text_state = remember(|| TextFieldState::new("copyable")).with(|state| *state);
    let zoom_state = remember(ZoomState::new).with(|state| *state);
    let wear_state = rememberWearScalingListState(CentreAnchor::default());
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Row(Modifier::empty(), RowSpec::default(), move || {
            ComposeBox(
                Modifier::empty()
                    .horizontal_scroll(scroll_state, false)
                    .press_interaction_source(interaction_source),
                BoxSpec::default(),
                move || {
                    Text(
                        format!(
                            "left {} {:.1} {}",
                            text_state.text(),
                            zoom_state.scale_non_reactive(),
                            wear_state.anchor().index
                        ),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
            ComposeBox(
                Modifier::empty()
                    .horizontal_scroll(scroll_state, false)
                    .press_interaction_source(interaction_source),
                BoxSpec::default(),
                move || {
                    Text(
                        format!(
                            "right {} {:.1} {}",
                            text_state.text(),
                            zoom_state.scale_non_reactive(),
                            wear_state.anchor().index
                        ),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
        });
    });
}

fn main() {
    robot_launch::launch("Reactive Handle Copy", 640, 240)
        .with_test_driver(|robot| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            robot.validate_content("left").expect("left content");
            robot.validate_content("right").expect("right content");
            robot.exit().expect("robot exit");
        })
        .run(reactive_handle_copy_screen);
}
