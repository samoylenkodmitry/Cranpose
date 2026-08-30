use std::{
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use cranpose::{AppLauncher, Robot};
use cranpose_testing::find_button_exact_in_semantics;

static FAILED: AtomicBool = AtomicBool::new(false);

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const MEASURE_FOR: Duration = Duration::from_millis(2000);
const WARMUP_FOR: Duration = Duration::from_millis(2000);
const WEAR_TAB: &str = "Wear (watch)";
const REFERENCE_TAB: &str = "Shader Rect";
const MAX_WEAR_WORK_MS: f32 = 2.5;
const MAX_REFERENCE_WORK_MS: f32 = 0.5;

struct Measured {
    fps: f32,
    work_fps: f32,
    work_avg_ms: f32,
}

fn measure(robot: &Robot, label: &str, window: Duration) -> Measured {
    let started = Instant::now();
    robot.reset_fps_stats().expect("reset fps stats");
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let stats = robot.fps_stats().expect("read fps stats");
    let measured = Measured {
        fps: stats.frame_count as f32 / elapsed.as_secs_f32(),
        work_fps: stats.work_fps,
        work_avg_ms: stats.work_avg_ms,
    };
    println!(
        "wear_fps stage={label} observed_fps={:.1} work_fps={:.1} work_avg_ms={:.3} \
         frames={} recomps={}",
        measured.fps,
        measured.work_fps,
        measured.work_avg_ms,
        stats.frame_count,
        stats.recomps_per_second,
    );
    measured
}

fn click_tab(robot: &Robot, label: &str) {
    let (x, y, w, h) = find_button_exact_in_semantics(robot, label)
        .unwrap_or_else(|| panic!("tab {label:?} not found"));
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .unwrap_or_else(|err| panic!("click tab {label:?}: {err}"));
    robot
        .pump_frames(3)
        .unwrap_or_else(|err| panic!("settle tab {label:?}: {err}"));
}

fn main() -> ExitCode {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Wear fps")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot.pump_frames(3).expect("build initial tab");

            click_tab(&robot, REFERENCE_TAB);
            measure(&robot, "shader-rect-warmup", WARMUP_FOR);
            let reference_before = measure(&robot, "shader-rect", MEASURE_FOR);

            click_tab(&robot, WEAR_TAB);
            measure(&robot, "wear-warmup", WARMUP_FOR);
            let wear = measure(&robot, "wear", MEASURE_FOR);

            click_tab(&robot, REFERENCE_TAB);
            let reference_after = measure(&robot, "shader-rect-after", MEASURE_FOR);
            let reference_ms = (reference_before.work_avg_ms + reference_after.work_avg_ms) * 0.5;
            let reference_drift =
                reference_after.work_avg_ms / reference_before.work_avg_ms.max(f32::EPSILON);

            let work_ratio = wear.work_avg_ms / reference_ms.max(f32::EPSILON);
            println!(
                "robot-metric: wear_fps summary wear={:.1}fps shader_rect={:.1}fps \
                 fps_ratio={:.2} wear_work_ms={:.3} shader_rect_work_ms={:.3} \
                 ref_drift={:.2} work_ratio={:.2}",
                wear.fps,
                reference_before.fps,
                wear.fps / reference_before.fps.max(1.0),
                wear.work_avg_ms,
                reference_ms,
                reference_drift,
                work_ratio,
            );
            robot.exit().ok();

            if wear.work_avg_ms >= MAX_WEAR_WORK_MS {
                println!(
                    "FAIL: the demo's heaviest text page costs {:.3}ms of CPU work per frame, \
                     over a ceiling of {MAX_WEAR_WORK_MS}ms. Something in the text pipeline, or \
                     in the per-row layer tree the scaling list builds, is re-deriving per frame \
                     what it should be reusing. (no-text reference: {:.3}ms, ratio {work_ratio:.1}x \
                     — printed for diagnosis, not judged)",
                    wear.work_avg_ms, reference_ms,
                );
                FAILED.store(true, Ordering::SeqCst);
                return;
            }
            if reference_ms >= MAX_REFERENCE_WORK_MS {
                println!(
                    "FAIL: the no-text reference page costs {reference_ms:.3}ms of CPU work per \
                     frame, over a ceiling of {MAX_REFERENCE_WORK_MS}ms. The baseline frame loop \
                     itself regressed — this page draws one shader rect."
                );
                FAILED.store(true, Ordering::SeqCst);
                return;
            }

            println!(
                "PASS: the heaviest text page costs {:.3}ms (ceiling {MAX_WEAR_WORK_MS}ms) and \
                 the no-text page {reference_ms:.3}ms (ceiling {MAX_REFERENCE_WORK_MS}ms); \
                 ratio {work_ratio:.1}x printed for trend only",
                wear.work_avg_ms,
            );
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");

    if FAILED.load(Ordering::SeqCst) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
