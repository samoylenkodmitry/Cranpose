use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Color, Modifier, composable,
    widgets::{Box, BoxSpec},
};

use crate::support;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 320;

#[composable]
fn ShadowedCardScene() {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.85, 0.86, 0.88, 1.0)),
        BoxSpec::new(),
        || {
            Box(
                Modifier::empty()
                    .size_points(200.0, 120.0)
                    .rounded_corners(12.0)
                    .shadow(6.0)
                    .background(Color(0.98, 0.98, 0.99, 1.0)),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

struct Harness {
    shell: AppShell<cranpose_render_wgpu::WgpuRenderer>,
}

impl Harness {
    fn new(renderer: cranpose_render_wgpu::WgpuRenderer) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let mut shell = AppShell::new(renderer, root_key, ShadowedCardScene);
        shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
        shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
        shell.update();
        Self { shell }
    }

    fn frame(&mut self) -> cranpose_render_wgpu::GpuPassTimingReport {
        self.shell.update();
        self.shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed");
        self.shell.renderer().gpu_pass_timings()
    }
}

fn adapter_can_time_passes() -> bool {
    let _lock = support::gpu_test_lock();
    let Ok(adapter) = support::device::headless_adapter(wgpu::Backends::all()) else {
        return false;
    };
    adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY)
}

struct TimingToggle;

impl TimingToggle {
    fn set() -> Self {
        cranpose_core::debug_toggles::set_debug_toggle("CRANPOSE_GPU_PASS_TIMING", Some("1"));
        Self
    }
}

impl Drop for TimingToggle {
    fn drop(&mut self) {
        cranpose_core::debug_toggles::set_debug_toggle("CRANPOSE_GPU_PASS_TIMING", None);
    }
}

#[test]
fn the_report_stays_empty_until_the_toggle_arms_timing() {
    let (_lock, renderer) = support::headless_renderer_parts().expect("headless renderer");
    let mut harness = Harness::new(renderer);
    for _ in 0..3 {
        let report = harness.frame();
        assert_eq!(report.frames, 0, "timing must be off by default");
        assert!(report.entries.is_empty(), "timing must be off by default");
    }
}

// The report after 32 captured frames with timing armed, compiling
// pipelines as `pipelines` says; None when the adapter cannot time passes.
fn timed_report(
    pipelines: support::device::Pipelines,
) -> Option<cranpose_render_wgpu::GpuPassTimingReport> {
    if !adapter_can_time_passes() {
        eprintln!("adapter lacks TIMESTAMP_QUERY; skipping pass-timing report test");
        return None;
    }
    let (_lock, _toggle, renderer) =
        support::headless_renderer_parts_configured(pipelines, TimingToggle::set)
            .expect("headless renderer");
    let mut harness = Harness::new(renderer);
    (0..32).map(|_| harness.frame()).last()
}

#[test]
fn a_frame_drawn_while_pipelines_compile_is_timed_whole_or_rejected_whole() {
    // A pass whose draws wait for a pipeline may leave a sample untaken,
    // as the slot's last frame left it.
    let Some(report) = timed_report(support::device::Pipelines::Background) else {
        return;
    };
    assert!(
        report.frames + report.invalid_frames >= 28,
        "query readback must keep up with captured frames: {report:?}"
    );
    assert_eq!(report.dropped_frames, 0, "{report:?}");
    for entry in &report.entries {
        assert_eq!(
            entry.passes,
            u64::from(report.frames),
            "a rejected frame adds none of its passes: {report:?}"
        );
    }
}

#[test]
fn a_rendered_scene_attributes_gpu_time_to_its_pass_labels() {
    // Pipelines compile where first needed, so every frame's passes draw:
    // Metal takes no end sample of a pass whose draws wait for a pipeline.
    let Some(report) = timed_report(support::device::Pipelines::Inline) else {
        return;
    };
    assert!(
        report.frames >= 28,
        "query readback must keep up with captured frames: {report:?}"
    );
    assert_eq!(report.invalid_frames, 0, "{report:?}");
    assert_eq!(report.dropped_frames, 0, "{report:?}");
    assert_eq!(report.dropped_passes, 0, "{report:?}");
    assert!(
        !report.entries.is_empty(),
        "a rendered frame must attribute at least one pass"
    );
    for entry in &report.entries {
        assert!(entry.passes > 0, "reported labels must have executed");
    }
    let output = report
        .entries
        .iter()
        .find(|entry| entry.label == "Output Conversion Pass")
        .expect("every captured frame must include its output conversion");
    assert_eq!(
        output.passes,
        u64::from(report.frames),
        "partial frames must not be counted as complete GPU timings: {report:?}"
    );
    let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
    assert!(
        total_ms > 0.0,
        "a frame with real draws must cost measurable GPU time; got {report:?}"
    );
}
