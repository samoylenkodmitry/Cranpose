//! The demo's heaviest text page must not cost a multiple of a page with none.
//!
//! Wear (watch) is the densest text in the demo: a scaling list of wrapped
//! rows on a 454dp round display, every row its own layer, scrolling
//! continuously so nothing settles into a still frame. Shader Rect is the same
//! renderer and the same loop with almost no text, so the ratio between them
//! is the text pipeline's per-frame cost with the rest of the frame divided
//! out.
//!
//! The verdict is **CPU work per frame**, not the frame rate: work is what the
//! framework controls, and it means the same thing on a machine that presents
//! in microseconds and one that presents in tens of milliseconds.
//!
//! # What this no longer covers
//!
//! It was written against the previous occupant of this tab -- a single
//! `Canvas` redrawing several hundred glyphs a frame, each at a size a curve
//! moved continuously -- to catch two ways of making that expensive: caching
//! glyph metrics against the size they were measured at, so an animated size
//! misses on every glyph of every frame, and evicting from the caches behind
//! them by scanning. Together those cost that page 13x its frame budget.
//!
//! The Wear widget set replaced it, and a scaling list scales a row through a
//! layer transform rather than by re-measuring its text, so **no font size
//! moves on this page any more** and neither defect would show here. That
//! specific regression is guarded where it is cheap and deterministic instead:
//! `a_font_size_animation_measures_each_glyph_once_rather_than_once_per_size`
//! in `cranpose-render-common`, which pins that measuring one string across 120
//! sizes adds no cache miss. What remains here is the broad one -- a text
//! pipeline, or a layer-per-row tree, that starts costing a multiple of a frame
//! with no text in it.

use std::{
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use cranpose::{AppLauncher, Robot};
use cranpose_testing::find_button_exact_in_semantics;

/// Set by the driver thread, read by `main` after the loop has torn down.
/// Exiting the process from the driver races the main thread's GPU teardown
/// and dumps core (exit 139), which buries the failure this is reporting.
static FAILED: AtomicBool = AtomicBool::new(false);

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const MEASURE_FOR: Duration = Duration::from_millis(2000);
/// Thrown away: a cold process spends its first seconds compiling GPU
/// pipelines and filling glyph caches.
const WARMUP_FOR: Duration = Duration::from_millis(2000);
const WEAR_TAB: &str = "Wear (watch)";
const REFERENCE_TAB: &str = "Shader Rect";
/// How much more CPU work per frame the demo's heaviest text page may cost
/// than a page with none.
///
/// Measured at 7.8x on this machine (0.504ms against 0.064ms), and the reason
/// is legible in the same reading: the page reports ~1799 recompositions a
/// second against ~1764 frames, so the list recomposes once per frame while it
/// scrolls. The reference page reports zero. Virtualising the list — composing
/// the rows it can show rather than the rows it has — moved this by less than
/// the host noise (7.67x before, 7.83x after), which is the tell that the cost
/// is the per-frame recomposition rather than the row count. The bound
/// sits above that with room for host noise rather than at it — this guards
/// against a text pipeline or a per-row layer tree that starts costing a
/// multiple of what it costs today, not against the scroll's own recomposition,
/// which is a known property of the widget set and is being raised separately.
///
/// The previous occupant of this tab measured 2x against the same reference
/// with `recomps=0`: a `Canvas` redrawing on a frame clock recomposes nothing.
/// Comparing the two numbers across the replacement is meaningless.
///
/// A cheaper *reference* also raises the ratio: the present-retained and
/// scroll-translate work moved the no-text page 0.036 → 0.031 ms on the CI
/// box (wear flat at 0.22-0.24 ms across six same-box reps), which is 6.5x
/// → 7.5x with no text regression anywhere. The bound has to leave room for
/// the denominator improving, and the two-sided reference below keeps
/// thermal drift from spending that room.
const MAX_WORK_RATIO: f32 = 12.0;

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

            // The reference again, on the far side of the wear stage. The
            // stages run in sequence on a box whose clock is allowed to sag
            // as it heats, so a single leading reference is measured at the
            // run's coolest moment and the wear stage at its hottest — a CI
            // box at 70°C read the reference 1.5x slower than a cool run
            // while the same binary's wear stage read 2.8x slower, and the
            // ratio blew through the bound with no code regression at all.
            //
            // Wear runs *between* the two references, so the midpoint
            // estimates the reference at wear's own thermal state. Not the
            // slower endpoint: that is the post-wear, hottest sample, and
            // dividing by it flips the bias from false alarms (loud,
            // investigated) to false passes (silent) — a real text
            // regression would be absorbed by the flattered denominator.
            click_tab(&robot, REFERENCE_TAB);
            let reference_after = measure(&robot, "shader-rect-after", MEASURE_FOR);
            let reference_ms = (reference_before.work_avg_ms + reference_after.work_avg_ms) * 0.5;
            // Drift magnitude: near 1.0 the box was thermally stable and the
            // ratio is trustworthy; well above it the run itself says it
            // should be retried rather than believed.
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

            if work_ratio >= MAX_WORK_RATIO {
                println!(
                    "FAIL: the demo's heaviest text page costs {work_ratio:.1}x the CPU work \
                     per frame of a page with none ({:.3}ms against {:.3}ms), over a bound of \
                     {MAX_WORK_RATIO:.0}x. Something in the text pipeline, or in the per-row \
                     layer tree the scaling list builds, is re-deriving per frame what it should \
                     be reusing.",
                    wear.work_avg_ms, reference_ms,
                );
                FAILED.store(true, Ordering::SeqCst);
                return;
            }

            println!(
                "PASS: the heaviest text page costs {work_ratio:.1}x a page with no text \
                 (bound {MAX_WORK_RATIO:.0}x)"
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
