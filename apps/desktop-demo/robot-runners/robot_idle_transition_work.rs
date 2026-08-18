use cranpose::AppLauncher;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_core::useState;
use cranpose_testing::find_button_in_semantics;
use cranpose_ui::{
    composable, Button, ButtonSpec, Column, ColumnSpec, LinearArrangement, Modifier, Text,
    TextStyle,
};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeMode {
    None,
    Unread,
    Consumed,
}

fn click_mode(robot: &cranpose::Robot, label: &str) {
    let (x, y, width, height) = find_button_in_semantics(robot, label)
        .unwrap_or_else(|| panic!("mode button {label:?} was not found"));
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .unwrap_or_else(|err| panic!("clicking mode button {label:?} failed: {err}"));
    std::thread::sleep(Duration::from_millis(350));
}

fn main() {
    env_logger::init();
    AppLauncher::new()
        .with_title("Idle Transition Work")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            click_mode(&robot, "Unread transition");
            robot.reset_fps_stats().expect("reset unread FPS stats");
            std::thread::sleep(Duration::from_millis(350));
            let unread = robot.fps_stats().expect("read unread FPS stats");
            let unread_runtime = robot
                .get_runtime_leak_debug_stats()
                .expect("read unread runtime stats");
            let unread_render = robot
                .get_render_stats()
                .expect("read unread render stats");
            println!(
                "IDLE-TRANSITION mode=unread frames={} recompositions={} work_fps={:.1} callbacks={} render={unread_render:?}",
                unread.frame_count,
                unread.recompositions,
                unread.work_fps,
                unread_runtime.runtime_stats.frame_callbacks_len,
            );

            robot.reset_fps_stats().expect("reset consumed FPS stats");
            click_mode(&robot, "Consumed transition");
            let consumed = robot.fps_stats().expect("read consumed FPS stats");
            let consumed_render = robot
                .get_render_stats()
                .expect("read consumed render stats");
            println!(
                "IDLE-TRANSITION mode=consumed frames={} recompositions={} work_fps={:.1} render={consumed_render:?}",
                consumed.frame_count,
                consumed.recompositions,
                consumed.work_fps,
            );

            assert_eq!(
                unread.frame_count, 0,
                "unread infinite transition scheduled unnecessary presented frames: {unread:?}"
            );
            assert_eq!(
                unread_runtime.runtime_stats.frame_callbacks_len, 0,
                "unread infinite transition left a frame callback queued: {unread_runtime:?}"
            );
            assert!(
                consumed.frame_count > 0 && consumed.recompositions > 0,
                "consumed infinite transition did not produce frame and composition work: {consumed:?}"
            );
            robot.exit().expect("exit idle transition work robot");
        })
        .run(probe_app);
}

#[composable]
#[allow(non_snake_case)]
fn probe_app() {
    let mode = useState(|| ProbeMode::None);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                "nothing on this screen moves".to_string(),
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                {
                    let mode = mode.clone();
                    move || mode.set(ProbeMode::Unread)
                },
                || {
                    Text("Unread transition", Modifier::empty(), TextStyle::default());
                },
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                {
                    let mode = mode.clone();
                    move || mode.set(ProbeMode::Consumed)
                },
                || {
                    Text(
                        "Consumed transition",
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
            match mode.get() {
                ProbeMode::None => {}
                ProbeMode::Unread => {
                    let _ = rememberInfiniteTransition("idle-probe").animateFloat(
                        0.0,
                        1.0,
                        infiniteRepeatable(
                            AnimationSpec::tween(900, Easing::EaseInOut),
                            RepeatMode::Reverse,
                            StartOffset::default(),
                        ),
                        "value",
                    );
                }
                ProbeMode::Consumed => {
                    let pulse = rememberInfiniteTransition("idle-probe").animateFloat(
                        0.0,
                        1.0,
                        infiniteRepeatable(
                            AnimationSpec::tween(900, Easing::EaseInOut),
                            RepeatMode::Reverse,
                            StartOffset::default(),
                        ),
                        "value",
                    );
                    Text(
                        format!("{:.2}", pulse.get()),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                }
            }
        },
    );
}
