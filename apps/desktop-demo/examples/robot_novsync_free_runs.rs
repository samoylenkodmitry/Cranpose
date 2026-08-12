//! Pressing NoVSync must actually free-run.
//!
//! Opens the Async Runtime tab, presses the NoVSync control in the dev overlay,
//! lets the app run for a wall-clock window, and asserts the observed frame
//! rate clears 120fps.
//!
//! Two things about this test are deliberate.
//!
//! It measures `fps`, not `work_fps`, and it never calls `pump_frames`. Pumping
//! frames forces the loop to produce them, which is exactly the thing under
//! test -- a pumped run reports healthy throughput no matter how badly the
//! pacing is broken, because the harness is supplying the frames the pacing
//! failed to ask for. The only honest measurement here is how many frames the
//! app produces on its own in a known stretch of wall-clock time.
//!
//! The threshold is 120 rather than something larger because the point is to
//! separate "free-running" from "pinned to the panel". A 60Hz display that
//! reports 60fps and a 120Hz one that reports 120 both mean the same failure:
//! the loop is following the display instead of running ahead of it. Async
//! Runtime is animated through `frame_clock().next_frame()`, so it is the page
//! that catches a pacing bug tied to frame callbacks.

use cranpose::{AppLauncher, FramePacingMode};
use cranpose_testing::find_text_in_semantics;
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
/// Long enough that a 60fps run and a free-running one cannot be confused, and
/// short enough to keep the example quick.
const MEASURE_FOR: Duration = Duration::from_millis(2000);
/// Thrown away. Long enough to get GPU pipeline compilation out of the way.
const WARMUP_FOR: Duration = Duration::from_millis(1500);
const MIN_FPS: f32 = 120.0;

/// Frames the app produced on its own, per second of wall clock.
fn measure(robot: &cranpose::Robot, window: Duration) -> f32 {
    let started = Instant::now();
    robot.reset_fps_stats().expect("reset fps stats");
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let stats = robot.fps_stats().expect("read fps stats");
    stats.frame_count as f32 / elapsed.as_secs_f32()
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("NoVSync free-run")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();

            // Open the tab whose animation is driven by frame callbacks.
            // Bounds arrive as (x, y, width, height).
            let (tab_x, tab_y, tab_w, tab_h) = find_text_in_semantics(&robot, "Async Runtime")
                .expect("the Async Runtime tab must be on screen");
            robot
                .click(tab_x + tab_w * 0.5, tab_y + tab_h * 0.5)
                .expect("click the Async Runtime tab");
            let _ = robot.wait_for_idle();
            std::thread::sleep(Duration::from_millis(300));

            // Press NoVSync where the overlay actually draws it.
            let (x, y) = robot
                .pacing_control_center(FramePacingMode::NoVsync)
                .expect("query the NoVSync control")
                .expect("the NoVSync control must be in the dev overlay");
            robot.click(x, y).expect("press NoVSync");
            let _ = robot.wait_for_idle();
            // Let the mode settle before the window opens: the surface is
            // reconfigured on the press, and the frames either side of that
            // belong to neither mode.
            std::thread::sleep(Duration::from_millis(400));

            // Discard a warm-up window before measuring. The first seconds of
            // a cold process are spent compiling GPU pipelines, and a window
            // that overlaps that measures the compiler, not the pacing: the
            // very first run of this example reported 60fps for exactly that
            // reason while every later run reported 400. Reporting both makes
            // a cold run visible instead of letting it decide the verdict.
            let warm = measure(&robot, WARMUP_FOR);
            println!("novsync_free_run stage=warmup observed_fps={warm:.1}");

            let (observed, stats, elapsed) = {
                let started = Instant::now();
                robot.reset_fps_stats().expect("reset fps stats");
                std::thread::sleep(MEASURE_FOR);
                let elapsed = started.elapsed();
                let stats = robot.fps_stats().expect("read fps stats");
                (
                    stats.frame_count as f32 / elapsed.as_secs_f32(),
                    stats,
                    elapsed,
                )
            };

            println!(
                "novsync_free_run stage=async_runtime fps={:.1} work_fps={:.1} \
                 avg_ms={:.2} frames={} over={:.2}s",
                stats.fps,
                stats.work_fps,
                stats.avg_ms,
                stats.frame_count,
                elapsed.as_secs_f32(),
            );

            println!("novsync_free_run observed_fps={observed:.1}");

            if observed <= MIN_FPS {
                println!(
                    "FAIL: NoVSync must free-run past {MIN_FPS:.0}fps, observed {observed:.1}fps \
                     ({} frames in {:.2}s). A result at or near the display's refresh rate means \
                     the loop is following the panel rather than running ahead of it.",
                    stats.frame_count,
                    elapsed.as_secs_f32(),
                );
                robot.exit().ok();
                std::process::exit(1);
            }

            println!("PASS: NoVSync free-ran at {observed:.1}fps");
            robot.exit().ok();
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");
}
