//! Robot test: capture real window screenshots of the Text showcase paragraph
//! sample while scrolling by fractional logical pixels per step and require the
//! paragraph crop to remain stable after alignment.

mod output_paths;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app::{self, DemoTab, TEST_ACTIVE_TAB_STATE};
use scroll_stability_external_helpers::{
    prepare_internal_diagnostic, run_scroll_stability_capture, ScrollStabilityConfig,
};
use std::time::Duration;
use text_showcase_external_helpers::{
    scroll_text_into_view_between, wait_for_text_showcase_heading,
};

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const WINDOW_TITLE: &str = "Robot Text Indent Scroll Stability";
const TARGET_TEXT: &str =
    "This paragraph demonstrates textIndent, lineHeight, lineBreak and hyphenation knobs in a constrained width box.";
const SCROLL_STEPS: usize = 12;
const SCROLL_DELTA_Y: f32 = -0.7;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
const COMPARE_MAX_ADJACENT_SCORE: u32 = 4;
const COMPARE_STABILIZED_GUARD_PX: u32 = 8;
const TARGET_MIN_CENTER_Y: f32 = 320.0;
const TARGET_MAX_CENTER_Y: f32 = 520.0;
const INTERNAL_DIAGNOSTIC_ENV: &str = "CRANPOSE_TEXT_INDENT_SCROLL_INTERNAL_DIAGNOSTIC";
const INTERNAL_DIAGNOSTIC_SCALE_ENV: &str = "CRANPOSE_TEXT_INDENT_SCROLL_INTERNAL_DIAGNOSTIC_SCALE";
const RENDER_STATS_ENV: &str = "CRANPOSE_TEXT_INDENT_SCROLL_RENDER_STATS";

fn main() {
    env_logger::init();
    println!("=== Robot Text Indent Scroll Stability ===");
    let internal_diagnostic = prepare_internal_diagnostic(
        INTERNAL_DIAGNOSTIC_ENV,
        INTERNAL_DIAGNOSTIC_SCALE_ENV,
        output_paths::diagnostic_path("cranpose_text_indent_scroll_internal"),
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
            scroll_text_into_view_between(
                &robot,
                TARGET_TEXT,
                TARGET_MIN_CENTER_Y,
                TARGET_MAX_CENTER_Y,
                32,
            );
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let compare_ok = run_scroll_stability_capture(
                &robot,
                ScrollStabilityConfig {
                    window_title: WINDOW_TITLE,
                    output_name: "text_indent_scroll_stability",
                    file_prefix: "text_indent_scroll",
                    target_text: TARGET_TEXT,
                    viewport_tag: Some(TARGET_TEXT),
                    window_width: WINDOW_WIDTH,
                    window_height: WINDOW_HEIGHT,
                    scroll_steps: SCROLL_STEPS,
                    scroll_delta_y: SCROLL_DELTA_Y,
                    step_epsilon: STEP_EPSILON,
                    fallback_trim_top_px: 200,
                    fallback_trim_bottom_px: 200,
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
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
            println!("PASS: text-indent paragraph stayed pixel-stable while scrolling");
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
