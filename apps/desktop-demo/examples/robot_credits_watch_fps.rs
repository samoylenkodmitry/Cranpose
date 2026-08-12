//! Text whose size animates must not cost more per frame than text that does not.
//!
//! Credits (watch) is a single `Canvas` that re-measures and re-draws several
//! hundred glyphs every frame, each at a size a scaling-list curve moves
//! continuously. Every other tab is a widget tree whose text is laid out once
//! and reused, so this is the one page whose frame cost is dominated by the
//! text pipeline, and the only one that shows what a moving font size does to
//! it.
//!
//! Two ways it has been made expensive, both of which this catches:
//! caching glyph metrics against the size they were measured at, which makes
//! every glyph of every frame a miss and a font-table read; and evicting from
//! the caches behind them by scanning for the least-recently-used entry, which
//! turns each of those misses into a walk of the whole table. Together they
//! cost this page 13x its frame budget.
//!
//! The verdict is **CPU work per frame**, not the frame rate: work is what the
//! framework controls, and it means the same thing on a machine that presents
//! in microseconds and one that presents in tens of milliseconds. The reference
//! is Shader Rect -- the same renderer and the same loop, with almost no text.

use cranpose::{AppLauncher, Robot};
use cranpose_testing::find_button_exact_in_semantics;
use std::time::{Duration, Instant};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const MEASURE_FOR: Duration = Duration::from_millis(2000);
/// Thrown away: a cold process spends its first seconds compiling GPU
/// pipelines and filling glyph caches.
const WARMUP_FOR: Duration = Duration::from_millis(2000);
const CREDITS_TAB: &str = "Credits (watch)";
const REFERENCE_TAB: &str = "Shader Rect";
/// How much more CPU work per frame a page of animated-size text may cost than
/// a page with none.
///
/// Measured on this machine it sits near 2x, windowed and headless alike. With
/// glyph metrics keyed by font size it was 10x windowed and 32x headless, so
/// anything at or above this bound is that class of regression rather than
/// ordinary drift.
const MAX_WORK_RATIO: f32 = 6.0;

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
        "credits_fps stage={label} observed_fps={:.1} work_fps={:.1} work_avg_ms={:.3} \
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
    std::thread::sleep(Duration::from_millis(260));
    let _ = robot.wait_for_idle();
}

fn main() {
    let _ = env_logger::try_init();

    AppLauncher::new()
        .with_title("Credits fps")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_fps_counter(true)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            let _ = robot.wait_for_idle();

            click_tab(&robot, REFERENCE_TAB);
            measure(&robot, "shader-rect-warmup", WARMUP_FOR);
            let reference = measure(&robot, "shader-rect", MEASURE_FOR);

            click_tab(&robot, CREDITS_TAB);
            measure(&robot, "credits-warmup", WARMUP_FOR);
            let credits = measure(&robot, "credits", MEASURE_FOR);

            let work_ratio = credits.work_avg_ms / reference.work_avg_ms.max(f32::EPSILON);
            println!(
                "credits_fps summary credits={:.1}fps shader_rect={:.1}fps fps_ratio={:.2} \
                 credits_work_ms={:.3} shader_rect_work_ms={:.3} work_ratio={:.2}",
                credits.fps,
                reference.fps,
                credits.fps / reference.fps.max(1.0),
                credits.work_avg_ms,
                reference.work_avg_ms,
                work_ratio,
            );
            robot.exit().ok();

            if work_ratio >= MAX_WORK_RATIO {
                println!(
                    "FAIL: a page of animated-size text costs {work_ratio:.1}x the CPU work per \
                     frame of a page with none ({:.3}ms against {:.3}ms), over a bound of \
                     {MAX_WORK_RATIO:.0}x. The text pipeline is re-deriving per frame what it \
                     should be reusing: glyph metrics scale linearly, so caching them against a \
                     size makes an animated size miss on every glyph, and a cache that evicts by \
                     scanning turns each miss into a walk of the whole table.",
                    credits.work_avg_ms, reference.work_avg_ms,
                );
                std::process::exit(1);
            }

            println!(
                "PASS: animated-size text costs {work_ratio:.1}x a still page's frame work \
                 (bound {MAX_WORK_RATIO:.0}x)"
            );
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch the demo");
}
