//! External X11 contract for Shader Rect animation cadence.
//!
//! This counts presented primary-window frames through desktop frame telemetry
//! and uses the robot driver only for deterministic teardown.

mod external_x11_frame_telemetry;
mod output_paths;
mod robot_perf_contract;
mod text_showcase_external_helpers;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::find_button_exact_in_semantics;
use desktop_app::app::{self, DemoTab};
use external_x11_frame_telemetry::{
    clear_records, install_primary_frame_telemetry_logger, summarize_records, FrameTelemetryRecord,
};
use image::RgbaImage;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use text_showcase_external_helpers::{capture_x11_window, find_window_id, focus_x11_window};

const WINDOW_TITLE: &str = "Robot Shader Rect External Animation";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 900;
const SAMPLE_DURATION_MS: u64 = 900;
const MIN_CHANGED_PIXELS: usize = 1_000;
const MIN_PRESENTED_FRAMES: usize = 96;
const MIN_PRESENTED_FPS: f64 = 120.0;
const MAX_P95_TOTAL_MS: f64 = 8.33;

fn main() {
    let records = install_primary_frame_telemetry_logger();
    let driver_records = Arc::clone(&records);

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            run_external_animation_driver(robot, driver_records);
        })
        .run(|| app::combined_app_with_initial_tab(Some(DemoTab::ShaderRect)));
}

fn run_external_animation_driver(robot: Robot, records: Arc<Mutex<Vec<FrameTelemetryRecord>>>) {
    let window_id = find_window_id(WINDOW_TITLE);
    focus_x11_window(&window_id);
    std::thread::sleep(Duration::from_millis(900));
    let _ = robot.wait_for_idle();

    let before_path = output_paths::diagnostic_path("shader_rect_external_animation_before.png");
    let before = capture_x11_window(&window_id, &before_path);

    clear_records(&records);
    let active_start = Instant::now();
    std::thread::sleep(Duration::from_millis(SAMPLE_DURATION_MS));
    let active_end = Instant::now();

    let after_path = output_paths::diagnostic_path("shader_rect_external_animation_after.png");
    let after = capture_x11_window(&window_id, &after_path);
    let changed = changed_pixel_count(&before, &after, 10);
    let summary = summarize_records(&records, active_start, active_end);

    println!(
        "external_x11_shader_rect_animation: changed_pixels={} frames={} fps={:.1} p95_total_ms={:.2} max_total_ms={:.2} p95_update_ms={:.2} p95_acquire_ms={:.2} p95_render_ms={:.2} p95_present_ms={:.2} stalls_50ms={} before={} after={}",
        changed,
        summary.frames,
        summary.fps,
        summary.p95_total_ms,
        summary.max_total_ms,
        summary.p95_update_ms,
        summary.p95_acquire_ms,
        summary.p95_render_ms,
        summary.p95_present_ms,
        summary.stalls_50ms,
        before_path.display(),
        after_path.display()
    );

    let mut failures = Vec::new();
    if changed < MIN_CHANGED_PIXELS {
        failures.push(format!(
            "Shader Rect animation did not visibly change: changed_pixels={changed}"
        ));
    }
    if robot_perf_contract::hardware_performance_contracts_enabled()
        && summary.frames < MIN_PRESENTED_FRAMES
    {
        failures.push(format!(
            "Shader Rect animation produced too few presented frames: frames={}",
            summary.frames
        ));
    }
    if robot_perf_contract::hardware_performance_contracts_enabled()
        && summary.fps < MIN_PRESENTED_FPS
    {
        failures.push(format!(
            "Shader Rect animation missed 120Hz presented cadence: fps={:.1}",
            summary.fps
        ));
    }
    if robot_perf_contract::hardware_performance_contracts_enabled()
        && summary.p95_total_ms > MAX_P95_TOTAL_MS
    {
        failures.push(format!(
            "Shader Rect animation p95 frame time exceeded 120Hz budget: p95_total_ms={:.2}",
            summary.p95_total_ms
        ));
    }
    if robot_perf_contract::hardware_performance_contracts_enabled() && summary.stalls_50ms > 0 {
        failures.push(format!(
            "Shader Rect animation produced {} frame stalls over 50ms",
            summary.stalls_50ms
        ));
    }
    if !robot_perf_contract::hardware_performance_contracts_enabled() {
        if summary.frames == 0 {
            failures.push("Shader Rect animation produced no presented frames".to_string());
        } else {
            robot_perf_contract::log_software_renderer_budget("Shader Rect animation");
        }
    }

    switch_to_idle_counter_tab(&robot, &window_id, &records);
    if !failures.is_empty() {
        eprintln!("FAIL: {}", failures.join("; "));
    }
    robot.exit().expect("exit");
}

fn switch_to_idle_counter_tab(
    robot: &Robot,
    window_id: &str,
    records: &Arc<Mutex<Vec<FrameTelemetryRecord>>>,
) {
    click_tab(robot, "Counter App");
    robot
        .wait_for_idle()
        .expect("Counter App should settle before exit telemetry");
    let after_static_path = output_paths::diagnostic_path("shader_rect_static_exit_after.png");
    let _ = capture_x11_window(window_id, &after_static_path);
    clear_records(records);
    let settle_start = Instant::now();
    std::thread::sleep(Duration::from_millis(900));
    let settle_end = Instant::now();
    let summary = summarize_records(records, settle_start, settle_end);
    println!(
        "shader_rect_static_exit_settle: frames={} fps={:.1} p95_total_ms={:.2} max_total_ms={:.2} screenshot={}",
        summary.frames,
        summary.fps,
        summary.p95_total_ms,
        summary.max_total_ms,
        after_static_path.display()
    );
}

fn click_tab(robot: &Robot, label: &str) {
    let Some((x, y, width, height)) = wait_for_tab(robot, label) else {
        panic!("tab button {label:?} was not found");
    };
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .unwrap_or_else(|err| panic!("failed to click tab {label:?}: {err}"));
}

fn wait_for_tab(robot: &Robot, label: &str) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..30 {
        if let Some(bounds) = find_button_exact_in_semantics(robot, label) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn changed_pixel_count(before: &RgbaImage, after: &RgbaImage, threshold: u8) -> usize {
    assert_eq!(before.dimensions(), after.dimensions());
    before
        .pixels()
        .zip(after.pixels())
        .filter(|(a, b)| {
            a.0.iter()
                .zip(b.0.iter())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .is_some_and(|delta| delta > threshold)
        })
        .count()
}
