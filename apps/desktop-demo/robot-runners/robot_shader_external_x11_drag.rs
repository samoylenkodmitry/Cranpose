//! External X11 contract for manual shader dragging.
//!
//! This runner does not install a Cranpose robot controller. It launches the
//! normal desktop event loop, drives the visible X11 window with `xdotool`, and
//! validates presented-frame telemetry during a held primary-pointer drag.

mod external_x11_frame_telemetry;
mod output_paths;
mod perf_contract;
mod text_showcase_external_helpers;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cranpose::AppLauncher;
use desktop_app::app::{self, DemoTab, ShaderSection, StartupSelection};
use external_x11_frame_telemetry::{
    clear_records, install_primary_frame_telemetry_logger, summarize_records, FrameTelemetryRecord,
};
use image::RgbaImage;
use text_showcase_external_helpers::{
    capture_x11_window, find_window_id, focus_x11_window, mouse_down_x11_button,
    mouse_up_x11_button, move_x11_mouse_in_window,
};

const WINDOW_TITLE: &str = "Robot Shader External X11 Drag";
const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 1000;
const START_X: f32 = 388.0;
const START_Y: f32 = 532.0;
const END_X: f32 = 284.0;
const END_Y: f32 = 470.0;
const MOVE_STEPS: usize = 42;
const HOLD_AFTER_MOVE_MS: u64 = 420;
const MIN_CHANGED_PIXELS: usize = 2_500;
const MIN_PRESENTED_FRAMES: usize = 48;
const MIN_PRESENTED_FPS: f64 = 150.0;
const MAX_P95_TOTAL_MS: f64 = 6.67;
const MAX_STALL_MS: f64 = 50.0;

fn main() {
    let records = install_primary_frame_telemetry_logger();

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_test_driver(move |robot| {
            run_external_drag_driver(Arc::clone(&records));
            robot.exit().expect("exit external shader drag app");
        })
        .run(|| {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Shaders),
                initial_shader_section: Some(ShaderSection::InteractiveEffects),
            });
        });
}

fn run_external_drag_driver(records: Arc<Mutex<Vec<FrameTelemetryRecord>>>) {
    let window_id = find_window_id(WINDOW_TITLE);
    focus_x11_window(&window_id);
    std::thread::sleep(Duration::from_millis(800));

    let before_path = output_paths::diagnostic_path("shader_external_x11_drag_before.png");
    let before = capture_x11_window(&window_id, &before_path);

    move_x11_mouse_in_window(&window_id, START_X, START_Y);
    std::thread::sleep(Duration::from_millis(120));
    clear_records(&records);

    mouse_down_x11_button("1");
    let active_start = Instant::now();
    for step in 1..=MOVE_STEPS {
        let t = step as f32 / MOVE_STEPS as f32;
        let x = START_X + (END_X - START_X) * t;
        let y = START_Y + (END_Y - START_Y) * t;
        move_x11_mouse_in_window(&window_id, x, y);
        std::thread::sleep(Duration::from_millis(3));
    }
    std::thread::sleep(Duration::from_millis(HOLD_AFTER_MOVE_MS));
    let active_end = Instant::now();
    mouse_up_x11_button("1");
    std::thread::sleep(Duration::from_millis(180));

    let after_path = output_paths::diagnostic_path("shader_external_x11_drag_after.png");
    let after = capture_x11_window(&window_id, &after_path);
    let changed = changed_pixel_count(&before, &after, 10);
    let summary = summarize_records(&records, active_start, active_end);

    println!(
        "external_x11_shader_drag: changed_pixels={} frames={} fps={:.1} p95_total_ms={:.2} max_total_ms={:.2} p95_update_ms={:.2} p95_acquire_ms={:.2} p95_render_ms={:.2} p95_present_ms={:.2} stalls_50ms={} before={} after={}",
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
            "drag did not visibly move shader content: changed_pixels={changed}"
        ));
    }
    if perf_contract::hardware_performance_contracts_enabled()
        && summary.frames < MIN_PRESENTED_FRAMES
    {
        failures.push(format!(
            "manual drag produced too few presented frames: frames={}",
            summary.frames
        ));
    }
    if perf_contract::hardware_performance_contracts_enabled() && summary.fps < MIN_PRESENTED_FPS {
        failures.push(format!(
            "manual drag missed {MIN_PRESENTED_FPS:.0}Hz presented-frame cadence: fps={:.1}",
            summary.fps,
        ));
    }
    if perf_contract::hardware_performance_contracts_enabled()
        && summary.p95_total_ms > MAX_P95_TOTAL_MS
    {
        failures.push(format!(
            "manual drag p95 frame time exceeded {MIN_PRESENTED_FPS:.0}Hz budget: p95_total_ms={:.2}",
            summary.p95_total_ms,
        ));
    }
    if perf_contract::hardware_performance_contracts_enabled() && summary.stalls_50ms > 0 {
        failures.push(format!(
            "manual drag produced {} frame stalls over {MAX_STALL_MS:.0}ms",
            summary.stalls_50ms
        ));
    }
    if !perf_contract::hardware_performance_contracts_enabled() {
        if summary.frames == 0 {
            failures.push("manual drag produced no presented frames".to_string());
        } else {
            perf_contract::log_software_renderer_budget("manual shader drag");
        }
    }

    if !failures.is_empty() {
        panic!("FAIL: {}", failures.join("; "));
    }
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
