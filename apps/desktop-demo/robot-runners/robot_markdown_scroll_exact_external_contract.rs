//! Robot test: capture real window screenshots of the Markdown list while scrolling by
//! exactly one logical pixel per step and require stable overlap inside the list viewport.

mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app;
use scroll_stability_external_helpers::{run_scroll_stability_capture, ScrollStabilityConfig};
use std::time::Duration;
use text_showcase_external_helpers::scroll_text_into_view_between;

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;
const WINDOW_TITLE: &str = "Robot Markdown Scroll Exact External";
const SCROLL_STEPS: usize = 10;
const SCROLL_DELTA_Y: f32 = -1.0;
const STEP_EPSILON: f32 = 0.05;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
const COMPARE_MAX_ADJACENT_SCORE: u32 = 4;
const COMPARE_STABILIZED_GUARD_PX: u32 = 32;
const COMPARE_VIEWPORT_INSET_PX: u32 = 200;
const TARGET_MIN_CENTER_Y: f32 = 220.0;
const TARGET_MAX_CENTER_Y: f32 = 320.0;
const RENDER_STATS_ENV: &str = "CRANPOSE_SCROLL_STABILITY_RENDER_STATS";

fn main() {
    env_logger::init();
    println!("=== Robot Markdown Scroll Exact External ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            scroll_text_into_view_between(
                &robot,
                app::MARKDOWN_SCROLL_STABILITY_TARGET_TEXT,
                TARGET_MIN_CENTER_Y,
                TARGET_MAX_CENTER_Y,
                48,
            );
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let compare_ok = run_scroll_stability_capture(
                &robot,
                ScrollStabilityConfig {
                    window_title: WINDOW_TITLE,
                    output_name: "markdown_scroll_exact_external",
                    file_prefix: "markdown_scroll",
                    target_text: app::MARKDOWN_SCROLL_STABILITY_TARGET_TEXT,
                    viewport_tag: Some("MarkdownListViewport"),
                    window_width: WINDOW_WIDTH,
                    window_height: WINDOW_HEIGHT,
                    scroll_steps: SCROLL_STEPS,
                    scroll_delta_y: SCROLL_DELTA_Y,
                    step_epsilon: STEP_EPSILON,
                    fallback_trim_top_px: 96,
                    fallback_trim_bottom_px: 96,
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                    compare_stabilized_guard_px: COMPARE_STABILIZED_GUARD_PX,
                    compare_viewport_inset_px: COMPARE_VIEWPORT_INSET_PX,
                    render_stats_env: Some(RENDER_STATS_ENV),
                },
                None,
            );
            if !compare_ok {
                std::process::exit(1);
            }

            println!("\n=== Test Summary ===");
            println!(
                "PASS: Markdown list content stayed pixel-identical in the overlapping region"
            );
            robot.exit().expect("exit");
        })
        .run(app::MarkdownScrollStabilityRobotApp);
}
