//! Pressing NoVSync must actually free-run. Currently it does not.
//!
//! Opens a continuously animating tab, presses the NoVSync control in the dev
//! overlay, and asserts the frame rate the app reaches on its own clears
//! 120fps. It reports ~60 instead.
//!
//! Four things about this test are deliberate, and each one is a way it was
//! wrong first.
//!
//! It turns OFF the robot's poll forcing around each measurement. A driven run
//! pins the event loop to `ControlFlow::Poll` so commands are serviced without
//! waiting on the display -- and polling is exactly the mechanism that lets a
//! loop outrun vsync. With it on, this test measured the harness free-running
//! and passed at 470fps against a build where pressing NoVSync does nothing.
//!
//! It never calls `pump_frames`, and reads `fps` rather than `work_fps`.
//! Pumping supplies the very frames the broken pacing failed to ask for, so a
//! pumped run reports healthy throughput however broken the pacing is.
//!
//! It measures several windows and judges on the WORST. The failure is
//! intermittent by nature -- `free_running_frame` is gated on `needs_redraw`,
//! so whether the loop keeps polling depends on whether pixels happen to be
//! stale when the control flow is chosen -- and a single window caught a good
//! streak and read 260fps on a clamped build.
//!
//! It measures a tab that animates forever on its own. A page that settles
//! reports 0fps once poll forcing is off, which is the idle-is-0fps invariant
//! working correctly and says nothing about pacing. Async Runtime looks like it
//! animates when a human is driving the window, but goes quiet under a robot.
//!
//! The threshold is 120 because the question is "free-running or pinned to the
//! panel", not "how fast". 60 on a 60Hz display and 120 on a 120Hz one are the
//! same failure.

use cranpose::{AppLauncher, FramePacingMode};
use cranpose_testing::find_text_in_semantics;
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
/// Long enough that a 60fps run and a free-running one cannot be confused, and
/// short enough to keep the example quick.
const MEASURE_FOR: Duration = Duration::from_millis(1500);
/// Windows measured; the worst one is the verdict.
const MEASURE_WINDOWS: usize = 3;
/// Thrown away. Long enough to get GPU pipeline compilation out of the way.
const WARMUP_FOR: Duration = Duration::from_millis(1500);
const MIN_FPS: f32 = 120.0;
/// Which tab to measure. It must animate continuously and on its own: a page
/// that settles reports 0fps once poll forcing is off, which is the idle
/// invariant working correctly and tells us nothing about pacing.
const TAB: &str = "Animations";

/// Frames the app produced on its own, per second of wall clock.
///
/// Poll forcing is off for the window. A robot-driven run otherwise pins the
/// event loop to `ControlFlow::Poll` so commands are serviced promptly, and
/// polling is precisely what lets a loop outrun vsync -- leaving it on makes
/// the harness free-run and reports a pass against a build where NoVSync does
/// nothing. It goes back on afterwards so the rest of the driver stays
/// responsive.
fn measure(robot: &cranpose::Robot, window: Duration) -> f32 {
    robot
        .set_poll_forcing(false)
        .expect("stop forcing poll for the measurement");
    let started = Instant::now();
    robot.reset_fps_stats().expect("reset fps stats");
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let stats = robot.fps_stats().expect("read fps stats");
    robot
        .set_poll_forcing(true)
        .expect("resume poll forcing after the measurement");
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
            let (tab_x, tab_y, tab_w, tab_h) = find_text_in_semantics(&robot, TAB)
                .expect("the tab under test must be on screen");
            robot
                .click(tab_x + tab_w * 0.5, tab_y + tab_h * 0.5)
                .expect("click the tab under test");
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

            // Several windows, and the verdict is the WEAKEST of them.
            //
            // The failure is intermittent by nature: `free_running_frame` is
            // gated on `needs_redraw`, so whether the loop keeps polling
            // depends on whether pixels happen to be stale at the instant the
            // control flow is chosen. One window can catch a good streak and
            // read 260fps on a build that spends most of its time clamped to
            // the panel. Taking the best -- or a single sample -- would make
            // this test pass on a broken build often enough to be useless. The
            // requirement is that NoVSync ALWAYS frees the loop, so the window
            // that did worst is the one that decides.
            let windows: Vec<f32> = (0..MEASURE_WINDOWS)
                .map(|_| measure(&robot, MEASURE_FOR))
                .collect();
            let observed = windows.iter().copied().fold(f32::INFINITY, f32::min);
            let best = windows.iter().copied().fold(0.0f32, f32::max);
            println!(
                "novsync_free_run windows={} worst={observed:.1} best={best:.1}",
                windows
                    .iter()
                    .map(|fps| format!("{fps:.1}"))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            let stats = robot.fps_stats().expect("read fps stats");
            let elapsed = MEASURE_FOR;

            println!(
                "novsync_free_run stage={TAB} fps={:.1} work_fps={:.1} \
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
                    "FAIL: NoVSync must free-run past {MIN_FPS:.0}fps in EVERY window, worst \
                     was {observed:.1}fps (best {best:.1}). A result at or near the display's \
                     refresh rate means the loop is following the panel rather than running \
                     ahead of it.",
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
