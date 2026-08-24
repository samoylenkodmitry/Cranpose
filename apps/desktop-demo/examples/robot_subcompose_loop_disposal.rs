#![allow(non_snake_case)]

use cranpose::AppLauncher;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable,
    widgets::{
        BoxWithConstraints, Button, ButtonSpec, CircularProgressIndicator, Column, ColumnSpec, Row,
        RowSpec, Text,
    },
    Modifier, TextStyle,
};
use cranpose_ui_graphics::Color;

#[composable]
fn BusyBadge() {
    Row(Modifier::empty(), RowSpec::new(), move || {
        CircularProgressIndicator(Modifier::empty(), Color(0.2, 0.5, 0.9, 1.0), 1.8);
        let pulse = rememberInfiniteTransition("recognizing").animateFloat(
            0.45,
            1.0,
            infiniteRepeatable(
                AnimationSpec::tween(900, Easing::EaseInOut),
                RepeatMode::Reverse,
                StartOffset::default(),
            ),
            "alpha",
        );
        Text(
            format!("busy {:.2}", pulse.get()),
            Modifier::empty(),
            TextStyle::default(),
        );
    });
}

#[composable]
fn NestedBadge() {
    BoxWithConstraints(Modifier::empty().fill_max_width(), move |_outer| {
        BoxWithConstraints(Modifier::empty().fill_max_width(), move |_inner| {
            BusyBadge();
        });
    });
}

#[composable]
fn ScreenBody(show_badge: MutableState<bool>) {
    if show_badge.get() {
        NestedBadge();
    } else {
        Text("another screen", Modifier::empty(), TextStyle::default());
    }
}

#[composable]
fn SubcomposeLoopDisposalScreen() {
    let show_badge = rememberMutableStateOf(|| true);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new(),
        move || {
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || show_badge.set_value(false),
                move || {
                    Text("leave", Modifier::empty(), TextStyle::default());
                },
            );
            ScreenBody(show_badge);
        },
    );
}

fn main() {
    AppLauncher::new()
        .with_title("subcompose loop disposal")
        .with_size(640, 480)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            robot.pump_frames(4).expect("compose the busy screen");
            robot
                .validate_content("busy")
                .expect("the nested badge is visible");
            let before = robot
                .get_runtime_leak_debug_stats()
                .expect("read runtime stats before leaving")
                .runtime_stats
                .tasks_len;

            robot.click_by_text("leave").expect("leave the badge screen");
            robot.pump_frames(4).expect("compose the replacement screen");
            robot
                .validate_content("another screen")
                .expect("the replacement screen is visible");
            let after = robot
                .get_runtime_leak_debug_stats()
                .expect("read runtime stats after leaving")
                .runtime_stats
                .tasks_len;

            robot.exit().ok();
            assert_eq!(
                after + 2,
                before,
                "leaving two nested subcompose layouts must stop both animation tasks: before={before}, after={after}"
            );
        })
        .run(SubcomposeLoopDisposalScreen);
}
