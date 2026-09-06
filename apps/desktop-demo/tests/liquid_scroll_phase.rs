#[path = "../../../crates/cranpose-render/wgpu/tests/support/device.rs"]
mod gpu_test_device;

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use desktop_app::app::{self, DemoTab, StartupSelection, TEST_LIQUID_SCROLL_STATE};

const LOGICAL_WIDTH: u32 = 800;
const LOGICAL_HEIGHT: u32 = 600;
const DENSITY: f32 = 130.0 / 96.0;
const BOTTOM_BAR_SCENE_OFFSET: f32 = 1423.154;
const PHYSICAL_PIXEL_STEPS: u32 = 10;
const EDGE_TRIM_LOGICAL: u32 = 180;
const MAX_CHANNEL_DELTA: u32 = 1;
const MAX_DIFFERING_PIXELS: u32 = 48;

fn physical(logical: u32) -> u32 {
    (logical as f32 * DENSITY).ceil() as u32
}

fn headless_renderer() -> Option<WgpuRenderer> {
    let device = gpu_test_device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Liquid Scroll Phase Test Device",
    )
    .ok()?;
    let mut renderer = WgpuRenderer::new(desktop_app::fonts::DEMO_FONTS);
    device.attach(&mut renderer, wgpu::TextureFormat::Bgra8UnormSrgb);
    Some(renderer)
}

fn capture(shell: &mut AppShell<WgpuRenderer>) -> Vec<u8> {
    shell.update();
    shell
        .renderer()
        .capture_frame_with_scale(physical(LOGICAL_WIDTH), physical(LOGICAL_HEIGHT), DENSITY)
        .expect("liquid page capture should succeed")
        .pixels
}

struct RasterDrift {
    differing_pixels: u32,
    worst_channel_delta: u32,
}

fn raster_drift(reference: &[u8], moved: &[u8]) -> RasterDrift {
    let mut drift = RasterDrift {
        differing_pixels: 0,
        worst_channel_delta: 0,
    };
    for (lhs, rhs) in reference
        .as_chunks::<4>()
        .0
        .iter()
        .zip(moved.as_chunks::<4>().0)
    {
        let delta = lhs
            .iter()
            .zip(rhs)
            .map(|(left, right)| u32::from(left.abs_diff(*right)))
            .max()
            .unwrap_or(0);
        if delta > 0 {
            drift.differing_pixels += 1;
            drift.worst_channel_delta = drift.worst_channel_delta.max(delta);
        }
    }
    drift
}

fn rows(frame: &[u8], first_row: u32, row_count: u32) -> &[u8] {
    let stride = (physical(LOGICAL_WIDTH) * 4) as usize;
    let start = first_row as usize * stride;
    &frame[start..start + row_count as usize * stride]
}

#[test]
fn the_liquid_page_at_the_bottom_bar_stays_exact_across_one_physical_pixel_scrolls() {
    let Some(renderer) = headless_renderer() else {
        eprintln!("skipping liquid scroll phase assertions: no headless GPU");
        return;
    };
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    let mut shell = AppShell::new_with_size_and_density(
        renderer,
        root_key,
        || {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Liquid),
                initial_shader_section: None,
            })
        },
        (physical(LOGICAL_WIDTH), physical(LOGICAL_HEIGHT)),
        (LOGICAL_WIDTH as f32, LOGICAL_HEIGHT as f32),
        DENSITY,
    );
    shell.update();
    shell.update();
    let scroll = TEST_LIQUID_SCROLL_STATE
        .with(|cell| *cell.borrow())
        .expect("the liquid page installs its scroll state");
    let context = shell.app_context().clone();
    context.enter(|| scroll.scroll_to(BOTTOM_BAR_SCENE_OFFSET));
    shell.update();
    let mut previous = capture(&mut shell);
    let edge = physical(EDGE_TRIM_LOGICAL);
    let compared_rows = physical(LOGICAL_HEIGHT) - edge * 2 - 1;
    let mut failures = Vec::new();
    for step in 1..=PHYSICAL_PIXEL_STEPS {
        let consumed = context.enter(|| scroll.dispatch_raw_delta(1.0 / DENSITY));
        assert!(
            (consumed - 1.0 / DENSITY).abs() < 1e-4,
            "step {step}: the page must consume one physical pixel, consumed {consumed}"
        );
        let current = capture(&mut shell);
        let drift = raster_drift(
            rows(&previous, edge + 1, compared_rows),
            rows(&current, edge, compared_rows),
        );
        eprintln!(
            "step {step}: offset={:.3} differing={} worst_channel_delta={}",
            scroll.value_non_reactive(),
            drift.differing_pixels,
            drift.worst_channel_delta
        );
        if drift.worst_channel_delta > MAX_CHANNEL_DELTA
            || drift.differing_pixels > MAX_DIFFERING_PIXELS
        {
            failures.push(format!(
                "step {step}: differing={} worst_channel_delta={}",
                drift.differing_pixels, drift.worst_channel_delta
            ));
        }
        previous = current;
    }
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    assert!(
        failures.is_empty(),
        "the liquid page scrolled by one physical pixel must move as one raster: {}",
        failures.join("; ")
    );
}
