//! Pressing a frame-pacing control has to change how fast the app runs.
//!
//! The app starts pinned to VSync, and the run presses its way through the dev
//! overlay's controls, measuring the frame rate the app reaches **on its own**
//! after each press. NoVSync must free the loop from the display; 60fps must
//! cap it; VSync must put the cap back.
//!
//! Five things about this test are deliberate.
//!
//! It **pins VSync at launch**. A robot harness otherwise lifts the pacing mode
//! to NoVSync for its own throughput measurements, so a test that starts in the
//! default mode is already in the mode it means to switch into and cannot tell
//! a working control from a dead one.
//!
//! It **presses the control the overlay actually drew**, at the centre the
//! shell reports. The overlay is renderer-drawn and carries no semantics; a
//! hard-coded coordinate starts hitting empty space the moment the overlay's
//! text changes, and a test that presses empty space passes.
//!
//! It **checks the 60fps cap as well as NoVSync**. A hard cap is an absolute
//! number that does not depend on the panel in front of it, so it is the one
//! reading that proves a press reached the loop at all. NoVSync alone cannot:
//! "fast" is also what a run does when nothing happened and it was already
//! free-running.
//!
//! It **never calls `pump_frames`, and reads `fps` rather than `work_fps`**.
//! Pumping supplies the very frames broken pacing failed to ask for, so a
//! pumped run reports healthy throughput however broken the pacing is.
//!
//! It **measures a tab that animates forever on its own**. A page that settles
//! reports 0fps, which is the idle-is-0fps invariant working correctly and says
//! nothing about pacing.
//!
//! What this cannot see, headless, is the swapchain: with no surface to present
//! to, a present mode that never left `Fifo` looks exactly like one that
//! free-runs. Run it windowed for that -- on a display that actually presents.
//! An unattended or virtual X server can take a full second per `Fifo` present
//! (`CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS=1` shows it as `present_ms≈998`),
//! which crawls every capped mode and reads as a pacing failure it is not.

use cranpose::{AppLauncher, FramePacingMode};
use cranpose_testing::find_text_in_semantics;
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
/// Long enough that a capped run and a free-running one cannot be confused, and
/// short enough to keep the example quick.
const MEASURE_FOR: Duration = Duration::from_millis(1500);
/// Thrown away. Long enough to get GPU pipeline compilation out of the way: the
/// first seconds of a cold process are spent compiling pipelines, and a window
/// that overlaps that measures the compiler rather than the pacing.
const WARMUP_FOR: Duration = Duration::from_millis(1500);
/// A hard cap is an absolute contract, so these are absolute bounds. The slack
/// is wide enough for a loaded machine to miss frames and still pass, and far
/// too narrow for an uncapped loop to sneak through.
const HARD60_MIN_FPS: f32 = 40.0;
const HARD60_MAX_FPS: f32 = 80.0;
/// Free-running is judged against the panel this run measured, not a number:
/// the capped stages read the display's own cadence, so twice that separates a
/// loop that broke away from one that is still following it, at 60Hz and at
/// 120Hz alike. Anchoring it to a constant would demand exactly the refresh
/// rate of a 120Hz display, which is the failure it is meant to catch.
const FREE_RUN_PANEL_MULTIPLE: f32 = 2.0;
/// Which tab to measure. It must animate continuously and on its own.
const TAB: &str = "Animations";

/// What one measurement window saw.
///
/// `fps` is the cadence the loop produced; `work_fps` is what the app could
/// have produced if nothing paced it. The pair is what separates a pacing
/// failure from a busy host: a clamped loop reads a low `fps` against a high
/// `work_fps`, while a machine that simply cannot keep up drags both down.
struct Measured {
    fps: f32,
    work_fps: f32,
}

/// Frames the app produced on its own, per second of wall clock.
///
/// Nothing here drives the loop. A robot with no command outstanding lets the
/// loop pace itself exactly as it would with no robot attached, which is what
/// makes this reading the app's rather than the harness's.
fn measure(robot: &cranpose::Robot, window: Duration) -> Measured {
    let started = Instant::now();
    robot.reset_fps_stats().expect("reset fps stats");
    std::thread::sleep(window);
    let elapsed = started.elapsed();
    let stats = robot.fps_stats().expect("read fps stats");
    Measured {
        fps: stats.frame_count as f32 / elapsed.as_secs_f32(),
        work_fps: stats.work_fps,
    }
}

/// Press a pacing control where the overlay draws it, and let the mode settle.
///
/// The surface is reconfigured on the press, and the frames either side of that
/// belong to neither mode.
fn press(robot: &cranpose::Robot, mode: FramePacingMode) {
    let (x, y) = robot
        .pacing_control_center(mode)
        .unwrap_or_else(|err| panic!("query the {} control: {err}", mode.label()))
        .unwrap_or_else(|| panic!("the {} control must be in the dev overlay", mode.label()));
    robot
        .click(x, y)
        .unwrap_or_else(|err| panic!("press {}: {err}", mode.label()));
    std::thread::sleep(Duration::from_millis(400));
}

fn fail(message: String) -> ! {
    println!("FAIL: {message}");
    std::process::exit(1);
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Frame pacing")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        // Explicit, so the robot harness does not lift the mode to NoVSync:
        // this run has to start capped to have anything to switch out of.
        .with_frame_pacing_mode(FramePacingMode::Vsync)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();

            // Bounds arrive as (x, y, width, height).
            let (tab_x, tab_y, tab_w, tab_h) =
                find_text_in_semantics(&robot, TAB).expect("the tab under test must be on screen");
            robot
                .click(tab_x + tab_w * 0.5, tab_y + tab_h * 0.5)
                .expect("click the tab under test");
            let _ = robot.wait_for_idle();
            std::thread::sleep(Duration::from_millis(300));

            let stage = |label: &str, window: Duration| {
                let measured = measure(&robot, window);
                println!(
                    "frame_pacing stage={label} observed_fps={:.1} work_fps={:.1}",
                    measured.fps, measured.work_fps
                );
                measured
            };

            stage("warmup", WARMUP_FOR);
            let vsync = stage("vsync", MEASURE_FOR);

            press(&robot, FramePacingMode::Hard60);
            let hard60 = stage("hard60", MEASURE_FOR);

            press(&robot, FramePacingMode::NoVsync);
            let free = stage("novsync", MEASURE_FOR);

            press(&robot, FramePacingMode::Vsync);
            let recapped = stage("vsync-again", MEASURE_FOR);

            robot.exit().ok();

            if !(HARD60_MIN_FPS..=HARD60_MAX_FPS).contains(&hard60.fps) {
                fail(format!(
                    "pressing 60fps must cap the loop near 60fps, measured {:.1}fps. A reading far \
                     above the cap means the press never reached the loop; far below means the app \
                     cannot keep up with its own cap (work_fps={:.1}).",
                    hard60.fps, hard60.work_fps,
                ));
            }

            // The panel's own cadence, as this run measured it rather than as a
            // constant: both capped stages follow the display.
            let panel_cadence = vsync.fps.max(hard60.fps);
            if free.fps <= panel_cadence * FREE_RUN_PANEL_MULTIPLE {
                fail(format!(
                    "pressing NoVSync must free the loop from the display: measured {:.1}fps \
                     against a panel cadence of {:.1}fps (work_fps={:.1}). A result at or near the \
                     refresh rate means the loop is still following the panel -- either waiting on \
                     it for the next redraw, or waiting on a swapchain that was never taken off \
                     vsync. A result well above the refresh rate but still far under work_fps is \
                     the cost of presenting in this environment rather than a loop that will not \
                     free-run: a software X server presents in tens of milliseconds.",
                    free.fps, panel_cadence, free.work_fps,
                ));
            }

            if recapped.fps >= free.fps * 0.5 {
                fail(format!(
                    "pressing VSync must put the cap back: measured {:.1}fps after {:.1}fps \
                     free-running (vsync baseline was {:.1}fps).",
                    recapped.fps, free.fps, vsync.fps,
                ));
            }

            println!(
                "PASS: vsync={:.1}fps 60fps-cap={:.1}fps novsync={:.1}fps vsync-again={:.1}fps",
                vsync.fps, hard60.fps, free.fps, recapped.fps,
            );
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");
}
