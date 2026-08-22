use cranpose::AppLauncher;
use cranpose_core::rememberMutableStateOf;
use cranpose_testing::find_button_in_semantics;
use cranpose_ui::widgets::{
    CircularProgressIndicator, CIRCULAR_INDICATOR_STROKE_WIDTH, PROGRESS_INDICATOR_COLOR,
};
use cranpose_ui::{composable, Button, ButtonSpec, Column, ColumnSpec, Modifier, Text, TextStyle};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static ROOT_COMPOSITIONS: AtomicUsize = AtomicUsize::new(0);

fn main() {
    env_logger::init();
    AppLauncher::new()
        .with_title("Draw-only Transition Work")
        .with_size(800, 600)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let (x, y, width, height) = find_button_in_semantics(&robot, "Activate")
                .expect("activate button was not found");
            robot
                .click(x + width * 0.5, y + height * 0.5)
                .expect("activate draw-only transition");
            std::thread::sleep(Duration::from_millis(350));
            robot.reset_fps_stats().expect("reset FPS stats");
            let root_before_interval = ROOT_COMPOSITIONS.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(350));
            let stats = robot.fps_stats().expect("read FPS stats");
            let runtime = robot
                .get_runtime_leak_debug_stats()
                .expect("read runtime stats");
            let root_after_interval = ROOT_COMPOSITIONS.load(Ordering::Relaxed);
            println!(
                "DRAW-ONLY-TRANSITION frames={} recompositions={} work_fps={:.1} callbacks={}",
                stats.frame_count,
                stats.recompositions,
                stats.work_fps,
                runtime.runtime_stats.frame_callbacks_len,
            );
            assert!(
                stats.frame_count > 0 && runtime.runtime_stats.frame_callbacks_len > 0,
                "draw-only infinite transition did not animate: {stats:?}"
            );
            assert_eq!(
                stats.recompositions, 0,
                "draw-only infinite transition caused composition work: {stats:?}"
            );
            assert_eq!(
                root_after_interval, root_before_interval,
                "draw-only infinite transition recomposed the root"
            );
            robot.exit().expect("exit draw-only transition robot");
        })
        .run(probe_app);
}

#[composable]
#[allow(non_snake_case)]
fn probe_app() {
    ROOT_COMPOSITIONS.fetch_add(1, Ordering::Relaxed);
    let active = rememberMutableStateOf(|| false);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || active.set(true),
            || {
                Text("Activate", Modifier::empty(), TextStyle::default());
            },
        );
        if active.get() {
            CircularProgressIndicator(
                Modifier::empty(),
                PROGRESS_INDICATOR_COLOR,
                CIRCULAR_INDICATOR_STROKE_WIDTH,
            );
        }
    });
}
