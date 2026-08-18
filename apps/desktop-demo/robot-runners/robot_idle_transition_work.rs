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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static ROOT_COMPOSITIONS: AtomicUsize = AtomicUsize::new(0);

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
            robot.screenshot().expect("capture baseline frame");
            let baseline_render = robot
                .get_render_stats()
                .expect("read baseline render stats")
                .expect("baseline render stats available");
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

            click_mode(&robot, "Consumed transition");
            robot.reset_fps_stats().expect("reset consumed interval stats");
            let root_after_mode = ROOT_COMPOSITIONS.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(350));
            let consumed = robot.fps_stats().expect("read consumed FPS stats");
            let consumed_runtime = robot
                .get_runtime_leak_debug_stats()
                .expect("read consumed runtime stats");
            let consumed_render = robot
                .get_render_stats()
                .expect("read consumed render stats")
                .expect("consumed render stats available");
            println!(
                "IDLE-TRANSITION mode=consumed frames={} recompositions={} work_fps={:.1} callbacks={} draws={} uploads={} layer_hits={} layer_misses={}",
                consumed.frame_count,
                consumed.recompositions,
                consumed.work_fps,
                consumed_runtime.runtime_stats.frame_callbacks_len,
                consumed_render.draw_calls,
                consumed_render.upload_bytes,
                consumed_render.layer_cache_hits,
                consumed_render.layer_cache_misses,
            );

            assert_eq!(
                unread.frame_count, 0,
                "unread infinite transition scheduled unnecessary presented frames: {unread:?}"
            );
            assert_eq!(
                unread.recompositions, 0,
                "unread infinite transition recomposed without a reader: {unread:?}"
            );
            assert_eq!(
                unread.work_fps, 0.0,
                "unread infinite transition performed frame work without a reader: {unread:?}"
            );
            assert_eq!(
                unread_runtime.runtime_stats.frame_callbacks_len, 0,
                "unread infinite transition left a frame callback queued: {unread_runtime:?}"
            );
            assert!(
                consumed.frame_count > 0
                    && consumed.recompositions > 0
                    && consumed_runtime.runtime_stats.frame_callbacks_len > 0,
                "consumed infinite transition did not produce frame and composition work: {consumed:?}"
            );
            assert!(
                ROOT_COMPOSITIONS.load(Ordering::Relaxed) > root_after_mode,
                "consumed transition did not recompose the subscribed root"
            );
            assert!(
                consumed_render.draw_calls == 0
                    && consumed_render.upload_bytes < baseline_render.upload_bytes
                    && consumed_render.layer_cache_hits > 0
                    && consumed_render.layer_cache_misses == 0,
                "consumed indicator expanded retained rendering work: baseline={baseline_render:?} consumed={consumed_render:?}"
            );
            robot.exit().expect("exit idle transition work robot");
        })
        .run(probe_app);
}

#[composable]
#[allow(non_snake_case)]
fn probe_app() {
    ROOT_COMPOSITIONS.fetch_add(1, Ordering::Relaxed);
    let mode = useState(|| ProbeMode::None);
    let transition = rememberInfiniteTransition("idle-probe");
    let pulse = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(900, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "value",
    );
    let pulse_value = (mode.get() == ProbeMode::Consumed).then(|| pulse.get());
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                "nothing on this screen moves".to_string(),
                Modifier::empty(),
                TextStyle::default(),
            );
            Text(
                "static retained content ".repeat(32),
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || mode.set(ProbeMode::Unread),
                || {
                    Text("Unread transition", Modifier::empty(), TextStyle::default());
                },
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || mode.set(ProbeMode::Consumed),
                || {
                    Text(
                        "Consumed transition",
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
            if let Some(pulse_value) = pulse_value {
                Text(
                    format!("{pulse_value:.2}"),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            }
        },
    );
}
