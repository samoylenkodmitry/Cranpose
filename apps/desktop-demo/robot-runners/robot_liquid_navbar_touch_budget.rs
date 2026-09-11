mod robot_exit;
mod robot_liquid_stage;
mod robot_shot;
mod robot_tab_fixture;

use std::{path::PathBuf, process::ExitCode, sync::atomic::AtomicBool, time::Duration};

use cranpose::{liquid::prelude::*, rememberMutableStateOf, AppLauncher, Modifier, Size};
use robot_liquid_stage::LiquidStripedStage;

const WINDOW_WIDTH: u32 = 880;
const WINDOW_HEIGHT: u32 = 260;
const TAB_WIDTH: f32 = 200.0;
const TAB_COUNT: usize = 4;
const BAR_HEIGHT: f32 = 64.0;
const BAR_TOP: f32 = 90.0;
const BAR_WIDTH: f32 = TAB_WIDTH * TAB_COUNT as f32;
const BAR_LEFT: f32 = (WINDOW_WIDTH as f32 - BAR_WIDTH) * 0.5;
const SETTLE_MS: u64 = 300;
const SETTLE_FRAMES: usize = 8;
const FRAMES_PER_TOUCH: u32 = 12;

/// A frame this long is a stall a person sees, not a slow frame. The freeze
/// reported against the demo's liquid navbar is several seconds long.
const FROZEN_FRAME_MS: f32 = 120.0;

/// What touching this bar may compile: nothing.
///
/// A settled bar has compiled every pipeline its picture needs, and a touch
/// changes uniforms, not shaders. On this platform every liquid material
/// shares one pipeline per blend mode, the way `controls.wgsl` always has,
/// so there is no material a press could reach that is not already built.
///
/// This was eight: per-material folds gave each touched material -- and the
/// lens that flies between tabs -- its own `override` set, compiled by the
/// backend inside the frame that first drew it, 514 ms for the first touch
/// and 408 ms for the next. `glass_material_folds_enabled` is the switch.
const PIPELINES_ALLOWED_ON_TOUCH: u64 = 0;

static FAILED: AtomicBool = AtomicBool::new(false);

const TABS: [(&str, &str); TAB_COUNT] = [
    (cranpose::liquid::icons::STAR, "Discover"),
    (cranpose::liquid::icons::LIST_OUTLINE, "Library"),
    (cranpose::liquid::icons::SCHEDULE, "Recent"),
    (cranpose::liquid::icons::SEARCH, "Search"),
];

fn main() -> ExitCode {
    let _ = env_logger::try_init();
    let shot_dir = PathBuf::from(
        std::env::var("ROBOT_SHOT_DIR")
            .unwrap_or_else(|_| "target/liquid-navbar-touch-budget".to_string()),
    );
    std::fs::create_dir_all(&shot_dir).expect("create shot dir");

    AppLauncher::new()
        .with_title("Liquid Navbar Touch Budget")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() != Ok("0"))
        .with_test_driver(move |robot| {
            robot_exit::arm_timeout(240);
            std::thread::sleep(Duration::from_millis(700));
            settle(&robot);

            // A headless app renders when it is asked for pixels, so every
            // step here ends in a screenshot: without one the touches are
            // recorded, the counters are read, and the frame that does the
            // work lands afterwards with nothing watching it.
            let resting_shot = robot.screenshot().expect("resting shot");
            robot_shot::save(&resting_shot, &shot_dir, "0-resting.png");
            robot.pump_frames(FRAMES_PER_TOUCH).expect("pump frames");
            let resting = robot.fps_stats().expect("resting fps stats");

            // Everything the resting bar draws is compiled by now. Whatever a
            // touch costs past this point is work the framework left until a
            // person asked for it.
            let mut built = cranpose::pipelines_created();
            report("resting", &resting, 0.0, built);

            let mut worst_ms = 0.0_f32;
            let mut worst_label = String::new();
            let mut compiled_on_touch = 0;
            for (index, cell) in [0usize, TAB_COUNT - 1, 1].into_iter().enumerate() {
                robot.reset_fps_stats().expect("reset fps stats");
                let started = std::time::Instant::now();
                robot
                    .click(cell_x(cell), CELL_Y)
                    .expect("click a navbar cell");
                robot.pump_frames(FRAMES_PER_TOUCH).expect("pump frames");
                let shot = robot.screenshot().expect("touch shot");
                let wall_ms = started.elapsed().as_secs_f32() * 1000.0;
                robot_shot::save(
                    &shot,
                    &shot_dir,
                    &format!("{}-touch-cell-{cell}.png", index + 1),
                );
                let stats = robot.fps_stats().expect("touch fps stats");
                let now_built = cranpose::pipelines_created();
                let label = format!("touch cell {cell}");
                report(&label, &stats, wall_ms, now_built);
                compiled_on_touch += now_built.saturating_sub(built);
                built = now_built;
                let stall_ms = stats.work_max_ms.max(stats.max_ms);
                if stall_ms > worst_ms {
                    worst_ms = stall_ms;
                    worst_label = label;
                }
                settle(&robot);
            }

            // The budget is what a person feels; the pipeline count is what
            // causes it. A driver that has compiled these shaders before
            // hands them back in a millisecond and the budget alone would
            // call a broken build healthy, so the count is the assertion
            // that holds whatever the driver cached.
            if compiled_on_touch > PIPELINES_ALLOWED_ON_TOUCH {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "touching the liquid navbar built {compiled_on_touch} pipelines, past \
                         the {PIPELINES_ALLOWED_ON_TOUCH} a touch may build. The bar was resting \
                         and settled first, so every one of these ran the backend's shader \
                         compiler inside the frame that drew the touch, and a person waits \
                         through all of them. Read the [pipeline-create] lines: a run of them \
                         differing only in `overrides=` is one pipeline per material."
                    ),
                );
            }

            if worst_ms > FROZEN_FRAME_MS {
                robot_exit::fail_and_await_shutdown(
                    &robot,
                    &FAILED,
                    &format!(
                        "{worst_label} stalled a frame for {worst_ms:.0}ms, past the \
                         {FROZEN_FRAME_MS:.0}ms a person reads as a freeze."
                    ),
                );
            }

            println!(
                "PASS: touching the liquid navbar built {compiled_on_touch} pipelines, within \
                 the {PIPELINES_ALLOWED_ON_TOUCH} it may, and its worst frame was \
                 {worst_ms:.1}ms, inside the {FROZEN_FRAME_MS:.0}ms budget"
            );
            robot.exit().expect("exit");
        })
        .try_run(move || {
            LiquidStripedStage(WINDOW_WIDTH, WINDOW_HEIGHT, move || {
                let selected = rememberMutableStateOf(|| 1usize);
                LiquidTabBar(
                    Modifier::empty()
                        .absolute_offset(BAR_LEFT, BAR_TOP)
                        .size(Size {
                            width: BAR_WIDTH,
                            height: BAR_HEIGHT,
                        }),
                    LiquidTabBarSpec::new(TAB_WIDTH),
                    selected.get(),
                    move |index| selected.set(index),
                    robot_tab_fixture::tabs(&TABS),
                );
            });
        })
        .map(|()| robot_exit::exit_code(&FAILED))
        .unwrap_or(ExitCode::FAILURE)
}

fn report(label: &str, stats: &cranpose::FpsStats, wall_ms: f32, pipelines: u64) {
    println!(
        "[navbar] {label}: wall={wall_ms:.1}ms fps={:.1} present(avg={:.2} max={:.2} p99={:.2}) \
         work(avg={:.2} max={:.2} p95={:.2}) stalled50ms={} frames={} \
         pipelines(in-frame={pipelines} off-frame={})",
        stats.fps,
        stats.avg_ms,
        stats.max_ms,
        stats.p99_ms,
        stats.work_avg_ms,
        stats.work_max_ms,
        stats.work_p95_ms,
        stats.work_stalled_50ms_frames,
        stats.interval_count,
        cranpose::pipelines_created_off_frame(),
    );
}

/// Runs the animations out without asking whether the app is idle: a liquid
/// control animates on its own, so `wait_for_idle` has nothing to return for
/// and the wait never ends.
fn settle(robot: &cranpose::Robot) {
    for _ in 0..SETTLE_FRAMES {
        robot.pump_frames(FRAMES_PER_TOUCH).expect("pump frames");
    }
    std::thread::sleep(Duration::from_millis(SETTLE_MS));
}

fn cell_x(cell: usize) -> f32 {
    BAR_LEFT + TAB_WIDTH * (cell as f32 + 0.5)
}

const CELL_Y: f32 = BAR_TOP + BAR_HEIGHT * 0.5;
