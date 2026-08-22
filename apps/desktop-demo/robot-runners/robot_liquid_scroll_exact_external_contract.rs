//! Real-window pixel-stability contract for the complete Liquid UI tab.
//!
//! Six independent scenes are scrolled by exactly one physical pixel per
//! frame. After compensating for that translation, every adjacent capture in
//! the shared viewport must remain pixel-stable. This catches coordinate
//! discontinuities shared by layout, ordinary draw layers, backdrop effects,
//! and runtime shaders instead of pinning one widget implementation.

mod output_paths;
mod scroll_stability_external_helpers;
mod text_showcase_external_helpers;

use cranpose::AppLauncher;
use desktop_app::app::{
    self, DemoTab, LIQUID_SCROLL_VIEWPORT_TAG, TEST_ACTIVE_TAB_STATE, TEST_LIQUID_SCROLL_STATE,
};
use scroll_stability_external_helpers::{
    advance_scroll_with_app_hook, run_presented_scroll_probe_with_app_hook,
    run_scroll_stability_capture_with_app_hook, semantics_bounds_for_exact_text,
    ExactScrollStepConfig, ScrollStabilityConfig,
};
use std::path::Path;
use std::time::Duration;
use text_showcase_external_helpers::scroll_text_into_view_between;

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;
const WINDOW_TITLE: &str = "Robot Liquid Scroll Exact External";
const SCROLL_STEPS: usize = 10;
const LOGICAL_PHASE_SWEEP_STEPS: usize = 10;
const STEP_EPSILON: f32 = 0.02;
const COMPARE_SEARCH_OFFSET_PX: u32 = 32;
// Total absolute RGBA delta across the full comparison crop. This permits at
// most 48 one-level 8-bit sampling differences among roughly 900,000 pixels;
// a moving edge or layer exceeds it by orders of magnitude.
const COMPARE_MAX_ADJACENT_SCORE: u32 = 48;

#[derive(Clone, Copy)]
struct LiquidScrollScene {
    name: &'static str,
    target_text: &'static str,
    target_center_range: (f32, f32),
    trim: (u32, u32, u32, u32),
}

const SCENES: [LiquidScrollScene; 6] = [
    LiquidScrollScene {
        name: "optical_shader_body",
        target_text: "wcKSRD OPTICS",
        // Keep the navigation title beyond its intentional 52dp collapse
        // transition so this scene isolates scrolling shader content.
        target_center_range: (280.0, 390.0),
        trim: (180, 180, 0, 0),
    },
    LiquidScrollScene {
        name: "optical_shader_left_edge",
        target_text: "wcKSRD OPTICS",
        target_center_range: (280.0, 390.0),
        // Keep the complete shader-card edge while excluding the
        // viewport-fixed floating tab bar from the translated comparison.
        trim: (180, 180, 0, 548),
    },
    LiquidScrollScene {
        name: "optical_shader_right_edge",
        target_text: "wcKSRD OPTICS",
        target_center_range: (280.0, 390.0),
        trim: (180, 180, 548, 0),
    },
    LiquidScrollScene {
        name: "bottom_bar_glass",
        target_text: "Custom Networking",
        target_center_range: (300.0, 560.0),
        trim: (180, 180, 0, 0),
    },
    LiquidScrollScene {
        name: "controls_card",
        target_text: "Wi-Fi",
        target_center_range: (300.0, 560.0),
        trim: (180, 180, 0, 0),
    },
    LiquidScrollScene {
        name: "segmented_control",
        target_text: "Receiving",
        target_center_range: (300.0, 560.0),
        trim: (180, 180, 0, 0),
    },
];

fn main() {
    env_logger::init();
    println!("=== Robot Liquid Scroll Exact External ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();
            robot
                .invoke_app_hook("set-tab", "liquid")
                .expect("select Liquid UI tab");
            settle(&robot);
            let density = robot
                .invoke_app_hook("liquid-density", "")
                .expect("read Liquid UI density")
                .expect("density hook response")
                .parse::<f32>()
                .expect("finite density response");
            assert!(
                density > 1.0,
                "fractional-density contract requires scale > 1, got {density}"
            );
            let physical_pixel_delta = -1.0 / density;
            println!(
                "density={density:.4} logical_delta_for_one_physical_pixel={physical_pixel_delta:.6}"
            );

            // Put both large shader cards across the viewport boundary. This
            // makes a one-row clip/surface seam visible without confusing it
            // with content legitimately entering the viewport.
            scroll_text_into_view_between(&robot, "wcKSRD OPTICS", 180.0, 230.0, 80);
            settle(&robot);
            let viewport = semantics_bounds_for_exact_text(&robot, LIQUID_SCROLL_VIEWPORT_TAG);
            assert!(
                run_presented_scroll_probe_with_app_hook(
                    &robot,
                    ScrollStabilityConfig {
                        window_title: WINDOW_TITLE,
                        output_name: "full_width_viewport_boundary",
                        file_prefix: "full_width_viewport_boundary",
                        target_text: "wcKSRD OPTICS",
                        viewport_tag: None,
                        window_width: WINDOW_WIDTH,
                        window_height: WINDOW_HEIGHT,
                        scroll_steps: LOGICAL_PHASE_SWEEP_STEPS,
                        scroll_delta_y: -1.0,
                        step_epsilon: STEP_EPSILON,
                        fallback_trim_top_px: 0,
                        fallback_trim_bottom_px: 0,
                        fallback_trim_left_px: 0,
                        fallback_trim_right_px: 0,
                        compare_search_offset_px: 0,
                        compare_max_adjacent_score: 0,
                        compare_max_channel_delta: 0,
                        compare_stabilized_guard_px: 0,
                        compare_viewport_inset_px: 0,
                        render_stats_env: None,
                        active_frame: None,
                    },
                    "scroll-liquid-content-by",
                    |step, path| {
                        viewport_boundary_has_no_full_width_seam(
                            step,
                            path,
                            viewport,
                            WINDOW_WIDTH,
                            WINDOW_HEIGHT,
                        )
                    },
                ),
                "Liquid viewport exposed a full-width boundary seam while scrolling"
            );

            let mut failed_scenes = Vec::new();
            let scene_filter = std::env::var("CRANPOSE_LIQUID_SCROLL_SCENE").ok();
            if let Some(filter) = scene_filter.as_deref() {
                assert!(
                    SCENES.iter().any(|scene| scene.name == filter),
                    "unknown Liquid scroll scene '{filter}'"
                );
            }
            for scene in SCENES {
                if scene_filter
                    .as_deref()
                    .is_some_and(|filter| filter != scene.name)
                {
                    continue;
                }
                println!(
                    "\n--- liquid scroll scene: {} ({}) ---",
                    scene.name, scene.target_text
                );
                scroll_text_into_view_between(
                    &robot,
                    scene.target_text,
                    scene.target_center_range.0,
                    scene.target_center_range.1,
                    80,
                );
                settle(&robot);

                // Cross multiple fractional-density rounding phases before
                // comparing complete scene crops. The viewport seam only
                // appears at particular logical scroll offsets.
                advance_scroll_with_app_hook(
                    &robot,
                    ExactScrollStepConfig {
                        target_text: scene.target_text,
                        window_width: WINDOW_WIDTH,
                        window_height: WINDOW_HEIGHT,
                        scroll_steps: LOGICAL_PHASE_SWEEP_STEPS,
                        scroll_delta_y: -1.0,
                        step_epsilon: STEP_EPSILON,
                        fallback_trim_top_px: scale_trim(scene.trim.0, density),
                        fallback_trim_bottom_px: scale_trim(scene.trim.1, density),
                    },
                    "scroll-liquid-content-by",
                );

                if !run_scroll_stability_capture_with_app_hook(
                    &robot,
                    ScrollStabilityConfig {
                        window_title: WINDOW_TITLE,
                        output_name: scene.name,
                        file_prefix: scene.name,
                        target_text: scene.target_text,
                        viewport_tag: None,
                        window_width: WINDOW_WIDTH,
                        window_height: WINDOW_HEIGHT,
                        scroll_steps: SCROLL_STEPS,
                        scroll_delta_y: physical_pixel_delta,
                        step_epsilon: STEP_EPSILON,
                        fallback_trim_top_px: scale_trim(scene.trim.0, density),
                        fallback_trim_bottom_px: scale_trim(scene.trim.1, density),
                        fallback_trim_left_px: scale_trim(scene.trim.2, density),
                        fallback_trim_right_px: scale_trim(scene.trim.3, density),
                        compare_search_offset_px: COMPARE_SEARCH_OFFSET_PX,
                        compare_max_adjacent_score: COMPARE_MAX_ADJACENT_SCORE,
                        compare_max_channel_delta: 1,
                        compare_stabilized_guard_px: 0,
                        compare_viewport_inset_px: 0,
                        render_stats_env: Some("CRANPOSE_LIQUID_SCROLL_RENDER_STATS"),
                        active_frame: None,
                    },
                    None,
                    "scroll-liquid-content-by",
                ) {
                    failed_scenes.push(scene.name);
                }
            }

            assert!(
                failed_scenes.is_empty(),
                "Liquid UI layers changed after normalized one-pixel scrolling in: {}",
                failed_scenes.join(", ")
            );
            println!("PASS: every Liquid UI scene stayed stable across exact 1px scrolls");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn settle(robot: &cranpose::Robot) {
    std::thread::sleep(Duration::from_millis(400));
    let _ = robot.wait_for_idle();
}

fn scale_trim(logical: u32, density: f32) -> u32 {
    (logical as f32 * density).round() as u32
}

fn viewport_boundary_has_no_full_width_seam(
    step: usize,
    path: &Path,
    viewport: (f32, f32, f32, f32),
    logical_window_width: u32,
    logical_window_height: u32,
) -> bool {
    let image = image::open(path)
        .unwrap_or_else(|err| panic!("failed to open {}: {err}", path.display()))
        .to_rgb8();
    let scale_x = image.width() as f32 / logical_window_width as f32;
    let scale_y = image.height() as f32 / logical_window_height as f32;
    let inset = (24.0 * scale_x).round() as u32;
    let left = (viewport.0 * scale_x).round().max(0.0) as u32 + inset;
    let right = ((viewport.0 + viewport.2) * scale_x)
        .round()
        .min(image.width() as f32) as u32
        - inset;
    assert!(left < right, "Liquid viewport boundary span is empty");

    let boundary = ((viewport.1 + viewport.3) * scale_y)
        .round()
        .clamp(2.0, image.height().saturating_sub(5) as f32) as u32;
    let mut seam = false;
    // Both shader cards cover every row immediately inside this boundary, so
    // their orange/purple split prevents any legitimate dominant full-width
    // color. This catches an exposed row regardless of its actual color.
    for row in boundary.saturating_sub(3)..boundary {
        let (color, fraction) = dominant_row_color(&image, left, right, row);
        println!(
            "viewport boundary step={step} row={row} dominant={color:?} fraction={fraction:.4}"
        );
        seam |= fraction >= 0.85;
    }
    !seam
}

fn dominant_row_color(image: &image::RgbImage, left: u32, right: u32, row: u32) -> ([u8; 3], f32) {
    let mut colors = (left..right)
        .map(|x| image.get_pixel(x, row).0)
        .collect::<Vec<_>>();
    colors.sort_unstable();
    let mut dominant = colors[0];
    let mut dominant_count = 1usize;
    let mut run_start = 0usize;
    for index in 1..=colors.len() {
        if index == colors.len() || colors[index] != colors[run_start] {
            let count = index - run_start;
            if count > dominant_count {
                dominant = colors[run_start];
                dominant_count = count;
            }
            run_start = index;
        }
    }
    (dominant, dominant_count as f32 / colors.len() as f32)
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name == "liquid-density" {
        return Ok(Some(cranpose_ui::current_density().to_string()));
    }
    if name == "scroll-liquid-content-by" {
        let content_delta = argument
            .parse::<f32>()
            .map_err(|err| format!("invalid liquid content delta '{argument}': {err}"))?;
        let scroll = TEST_LIQUID_SCROLL_STATE
            .with(|cell| *cell.borrow())
            .ok_or_else(|| "liquid scroll state was not installed".to_string())?;
        let consumed = scroll.dispatch_raw_delta(-content_delta);
        return Ok(Some(format!(
            "content_delta={content_delta:.3} consumed={consumed:.3} offset={:.3}",
            scroll.value_non_reactive()
        )));
    }
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "liquid" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
    state.set(DemoTab::Liquid);
    Ok(None)
}
