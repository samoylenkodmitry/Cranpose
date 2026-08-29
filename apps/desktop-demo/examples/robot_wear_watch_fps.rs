//! The demo's heaviest text page must not become pathologically expensive.
//!
//! Wear (watch) is the densest text in the demo: a scaling list of wrapped
//! rows on a 454dp round display, every row its own layer, scrolling
//! continuously so nothing settles into a still frame. Shader Rect is the same
//! renderer and the same loop with almost no text. Both pages are judged
//! against ABSOLUTE per-frame CPU-work ceilings; their ratio is printed as a
//! diagnostic but never judged, because a ratio of two sub-millisecond
//! timings fails perf improvements (a cheaper baseline raises it) and fails
//! hot shared hosts (the two stages do not scale together under load).
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
/// Absolute ceiling on the heaviest text page's CPU work per frame.
///
/// This was a ratio bound (wear over the no-text reference, 12x) until the
/// ratio failed two perf PRs in one morning for succeeding: it divides two
/// sub-millisecond numbers, so removing per-frame overhead shrinks the
/// denominator faster than the numerator and a genuine improvement pushes
/// the ratio UP — the present-retained work alone moved it 6.5x → 7.5x with
/// no text regression anywhere. On a shared box under load the two stages
/// do not scale together either (79°C, load 12/12 read the reference 1.5x
/// slow and wear 2.8x slow in one run). Two absolute ceilings keep the
/// property the ratio protected — text must not become pathologically
/// expensive — while surviving both the baseline getting cheaper and the
/// box being hot.
///
/// Samples: 0.22-0.24 ms across six same-box CI reps, 0.504 ms on a dev
/// machine, 0.786 ms on the CI box at load 72. The defect class this
/// guards (per-frame glyph re-measure/cache scans, a per-row layer tree
/// gone quadratic) costs an order of magnitude, not tens of percent, so
/// the ceiling sits at ~10x the quiet-box reading and ~3x the worst
/// load-inflated sample ever observed.
const MAX_WEAR_WORK_MS: f32 = 2.5;
/// Absolute ceiling on the no-text reference page, so the baseline itself
/// cannot silently regress now that nothing divides by it. Samples:
/// 0.031-0.064 ms quiet, 1.5x that at 70°C.
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
