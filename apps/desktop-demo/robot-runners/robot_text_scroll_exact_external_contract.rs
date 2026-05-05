//! Robot test: capture REAL window screenshots of the Text showcase while scrolling by
//! exactly one logical pixel per step and require perfect overlap in the shared middle area.

mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app;
use scroll_stability_external_helpers::{
    prepare_internal_diagnostic, run_scroll_stability_capture, ScrollStabilityConfig,
};
use std::path::PathBuf;
use std::time::Duration;
use text_showcase_external_helpers::{open_text_tab, scroll_text_into_view};

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
const COMPARE_MAX_ADJACENT_SCORE: u32 = 4;
const INTERNAL_DIAGNOSTIC_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC";
const INTERNAL_DIAGNOSTIC_SCALE_ENV: &str = "CRANPOSE_TEXT_SCROLL_INTERNAL_DIAGNOSTIC_SCALE";
const RENDER_STATS_ENV: &str = "CRANPOSE_TEXT_SCROLL_RENDER_STATS";

fn main() {
    env_logger::init();
    println!("=== Robot Text Scroll Exact External ===");
    let internal_diagnostic = prepare_internal_diagnostic(
        INTERNAL_DIAGNOSTIC_ENV,
        INTERNAL_DIAGNOSTIC_SCALE_ENV,
        PathBuf::from("/tmp/cranpose_text_scroll_exact_internal"),
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
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(1000));
            let _ = robot.wait_for_idle();

            open_text_tab(&robot);
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
                    compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                    compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                    compare_viewport_inset_px: 0,
                    render_stats_env: Some(RENDER_STATS_ENV),
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
