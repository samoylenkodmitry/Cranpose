use std::time::Duration;

use cranpose::AppLauncher;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_testing::find_button_in_semantics;
use cranpose_ui::{
    composable,
    widgets::{
        Button, ButtonSpec, CircularProgressIndicator, Column, ColumnSpec, LazyColumn,
        LazyColumnSpec, Spacer, Text, PROGRESS_INDICATOR_COLOR,
    },
    Modifier, Size, TextStyle,
};

fn main() {
    env_logger::init();
    AppLauncher::new()
        .with_title("Offscreen Draw Transition")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let initial = robot
                .get_runtime_leak_debug_stats()
                .expect("read initial runtime stats");
            assert!(
                initial.runtime_stats.frame_callbacks_len > 0,
                "visible draw-only transition did not start: {initial:?}"
            );

            let (x, y, width, height) =
                find_button_in_semantics(&robot, "Jump").expect("jump button was not found");
            robot
                .click(x + width * 0.5, y + height * 0.5)
                .expect("jump lazy list");
            std::thread::sleep(Duration::from_millis(900));

            let offscreen = robot
                .get_runtime_leak_debug_stats()
                .expect("read offscreen runtime stats");
            assert_eq!(
                offscreen.runtime_stats.frame_callbacks_len, 0,
                "offscreen retained draw-only transition is still scheduled: {offscreen:?}"
            );
            robot.exit().expect("exit offscreen draw transition robot");
        })
        .run(probe_app);
}

#[composable]
#[allow(non_snake_case)]
fn probe_app() {
    let state = rememberLazyListState();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || state.scroll_to_item(99, 0.0),
                || {
                    Text("Jump", Modifier::empty(), TextStyle::default());
                },
            );
            LazyColumn(
                Modifier::empty().fill_max_size(),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(100, |index| {
                        if index == 0 {
                            CircularProgressIndicator(
                                Modifier::empty(),
                                PROGRESS_INDICATOR_COLOR,
                                4.0,
                            );
                        } else {
                            Spacer(Size {
                                width: 0.0,
                                height: 64.0,
                            });
                        }
                    });
                },
            );
        },
    );
}
