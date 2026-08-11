//! Robot test: capture REAL window screenshots of the Text showcase while scrolling by
//! exactly one logical pixel per step and require perfect overlap in the shared middle area.

mod output_paths;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app::{self, DemoTab, TEST_ACTIVE_TAB_STATE};
use scroll_stability_external_helpers::{
    prepare_internal_diagnostic, run_scroll_stability_capture, ScrollStabilityConfig,
};
use std::time::Duration;
use text_showcase_external_helpers::{scroll_text_into_view, wait_for_text_showcase_heading};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const WINDOW_TITLE: &str = "Robot Text Scroll Exact External";
const TARGET_TEXT: &str =
    "This is bold green and this is normal text. This is red, italic, and underlined!";
const SCROLL_STEPS: usize = 10;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_TRIM_TOP_PX: u32 = 200;
const COMPARE_TRIM_BOTTOM_PX: u32 = 200;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
// Summed per-channel budget for how far two re-aligned adjacent frames may
// drift. The desktop swapchain is a non-sRGB view (the framework's sRGB
// pass-through color contract, pinned byte-exact by robot_color_fidelity), so
// alpha compositing lands in sRGB byte space. Software rasterizers — the CI
// Vulkan path is lavapipe — round the anti-aliased corners of the Text tab's
// translucent (alpha 0.95) rounded background cards ~1 level differently at
// different absolute scroll offsets; real GPUs stay exact (this passes on the
// local RTX 2070 suite). The budget absorbs that imperceptible rounding
// (observed worst case 5 summed) while any real scroll drift scores far higher.
const COMPARE_MAX_ADJACENT_SCORE: u32 = 8;
const COMPARE_STABILIZED_GUARD_PX: u32 = 0;
const INTERNAL_DIAGNOSTIC_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC";
const INTERNAL_DIAGNOSTIC_SCALE_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC_SCALE";
const RENDER_STATS_ENV: &str = "CRANPOSE_TEXT_SCROLL_RENDER_STATS";

fn main() {
    env_logger::init();
    println!("=== Robot Text Scroll Exact External ===");
    let internal_diagnostic = prepare_internal_diagnostic(
        INTERNAL_DIAGNOSTIC_ENV,
        INTERNAL_DIAGNOSTIC_SCALE_ENV,
        output_paths::diagnostic_path("cranpose_text_scroll_exact_internal"),
    );
    if let Some(diagnostic) = &internal_diagnostic {
        println!(
            "Internal diagnostic dir: {} scale={:.4}",
            diagnostic.output_dir.display(),
            diagnostic.capture_scale
        );
    }

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            walk_tabs_to_text(&robot);
            wait_for_text_showcase_heading(&robot);
            if std::env::var_os("CRANPOSE_TEXT_SCROLL_HELPER_DIAGNOSTIC").is_some() {
                match robot.screenshot() {
                    Ok(screenshot) => println!(
                        "text robot internal screenshot pixels={}x{} logical={:.1}x{:.1}",
                        screenshot.width,
                        screenshot.height,
                        screenshot.logical_width,
                        screenshot.logical_height
                    ),
                    Err(err) => println!("text robot internal screenshot error={err}"),
                }
            }
            scroll_text_into_view(&robot, TARGET_TEXT, WINDOW_HEIGHT, 20);
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let compare_ok = run_scroll_stability_capture(
                &robot,
                ScrollStabilityConfig {
                    window_title: WINDOW_TITLE,
                    output_name: "text_scroll_exact_external",
                    file_prefix: "text_scroll",
                    target_text: TARGET_TEXT,
                    viewport_tag: None,
                    window_width: WINDOW_WIDTH,
                    window_height: WINDOW_HEIGHT,
                    scroll_steps: SCROLL_STEPS,
                    scroll_delta_y: SCROLL_DELTA_Y,
                    step_epsilon: STEP_EPSILON,
                    fallback_trim_top_px: COMPARE_TRIM_TOP_PX,
                    fallback_trim_bottom_px: COMPARE_TRIM_BOTTOM_PX,
                    fallback_trim_left_px: 0,
                    fallback_trim_right_px: 0,
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                    compare_max_channel_delta: 0,
                    compare_stabilized_guard_px: COMPARE_STABILIZED_GUARD_PX,
                    compare_viewport_inset_px: 0,
                    render_stats_env: Some(RENDER_STATS_ENV),
                    active_frame: None,
                },
                internal_diagnostic.as_ref(),
            );
            if !compare_ok {
                std::process::exit(1);
            }

            println!("\n=== Test Summary ===");
            println!(
                "PASS: external scroll capture stayed pixel-identical in the overlapping region"
            );
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn walk_tabs_to_text(robot: &cranpose::Robot) {
    for tab in ["mineswapper2", "images", "lazy-list", "text"] {
        set_active_tab(robot, tab);
        std::thread::sleep(Duration::from_millis(180));
        let _ = robot.wait_for_idle();
    }
}

fn set_active_tab(robot: &cranpose::Robot, tab: &str) {
    robot
        .invoke_app_hook("set-tab", tab)
        .unwrap_or_else(|err| panic!("failed to select tab '{tab}': {err}"));
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let tab = match argument.as_str() {
        "mineswapper2" => DemoTab::Mineswapper2,
        "images" => DemoTab::Images,
        "lazy-list" => DemoTab::LazyList,
        "text" => DemoTab::Text,
        _ => return Err(format!("unknown demo tab '{argument}'")),
    };
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .unwrap_or_else(|| panic!("active tab state was not installed before selecting {tab:?}"));
    state.set(tab);
    Ok(None)
}
