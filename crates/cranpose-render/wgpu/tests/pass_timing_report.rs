mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_ui::{
    Color, Modifier, composable,
    widgets::{Box, BoxSpec},
};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 320;

#[composable]
#[allow(non_snake_case)]
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
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        return false;
    };
    adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY)
}

struct TimingToggle;

impl TimingToggle {
    fn set() -> Self {
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_GPU_PASS_TIMING", Some("1"));
        Self
    }
}

impl Drop for TimingToggle {
    fn drop(&mut self) {
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_GPU_PASS_TIMING", None);
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

#[test]
fn a_rendered_scene_attributes_gpu_time_to_its_pass_labels() {
    if !adapter_can_time_passes() {
        eprintln!("adapter lacks TIMESTAMP_QUERY; skipping pass-timing report test");
        return;
    }
    let (_lock, _toggle, renderer) =
        support::headless_renderer_parts_configured(TimingToggle::set).expect("headless renderer");
    let mut harness = Harness::new(renderer);

    let mut report = harness.frame();
    for _ in 0..20 {
        if report.frames > 0 {
            break;
        }
        report = harness.frame();
    }

    assert!(
        report.frames > 0,
        "no timed frame was read back within 21 frames"
    );
    assert!(
        !report.entries.is_empty(),
        "a rendered frame must attribute at least one pass"
    );
    for entry in &report.entries {
        assert!(entry.passes > 0, "reported labels must have executed");
    }
    let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
    assert!(
        total_ms > 0.0,
        "a frame with real draws must cost measurable GPU time; got {report:?}"
    );
}
