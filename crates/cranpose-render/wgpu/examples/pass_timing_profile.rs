//! Headless GPU pass-timing profile of the shadowed-card scroll — the
//! document-list workload behind issue #500 — at handset resolution.
//!
//! The Huawei Mate 20 X target cannot time passes at all (its Mali-G76
//! Vulkan driver reports `timestampValidBits = 0` on every queue), so the
//! attribution loop runs here on Metal — a tile-based GPU like the target —
//! and the device stays the fps gate. Run with:
//!
//! ```sh
//! CRANPOSE_GPU_PASS_TIMING=1 cargo run --release -p cranpose-render-wgpu \
//!     --example pass_timing_profile
//! ```

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    Color, LinearArrangement, Modifier, RenderEffect, composable,
    widgets::{Box, BoxSpec, LazyColumn, LazyColumnSpec},
};
use cranpose_ui_graphics::{LiquidGlassRect, LiquidGlassSpec, TileMode, liquid_glass_effect};

const FRAME_WIDTH: u32 = 1080;
const FRAME_HEIGHT: u32 = 2244;
const WARMUP_FRAMES: usize = 60;
const MEASURED_FRAMES: usize = 240;
const SCROLL_DELTA_PER_FRAME: f32 = -30.0;

#[composable]
#[allow(non_snake_case)]
fn CardRow(index: usize) {
    let fill = if index.is_multiple_of(2) {
        Color(0.98, 0.98, 0.99, 1.0)
    } else {
        Color(0.94, 0.95, 0.97, 1.0)
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(280.0)
            .rounded_corners(24.0)
            .shadow(12.0)
            .background(fill),
        BoxSpec::new(),
        || {},
    );
}

fn chrome_blur_radius() -> f32 {
    std::env::var("PROFILE_BLUR_RADIUS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(36.0)
}

/// The glass-material effect chain cranscan's fixed chrome builds: a
/// separable Gaussian pre-blur feeding the liquid-glass optical shader.
fn chrome_glass_effect(rect_width: f32, rect_height: f32) -> RenderEffect {
    let optical = liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: rect_width,
            height: rect_height,
            tint_color: Color(0.9, 0.9, 0.95, 0.35),
        },
        &LiquidGlassSpec {
            corner_radius: 28.0,
            blur_radius: 0.0,
            ..LiquidGlassSpec::default()
        },
        rect_width,
        rect_height,
    );
    RenderEffect::blur_with_edge_treatment(chrome_blur_radius(), TileMode::Mirror).then(optical)
}

/// Shaped like the device chrome: shape clip + shadow + own content, so the
/// bar renders as a child layer surface with a backdrop, exactly the
/// uncacheable topology cranscan's toolbar and tab bar force each frame.
#[composable]
#[allow(non_snake_case)]
fn GlassBar(x: f32, y: f32, width: f32, height: f32) {
    Box(
        Modifier::empty()
            .offset(x, y)
            .width(width)
            .height(height)
            .rounded_corners(28.0)
            .shadow(8.0)
            .backdrop_effect(chrome_glass_effect(width, height))
            .padding(24.0),
        BoxSpec::new(),
        || {
            Box(
                Modifier::empty()
                    .width(180.0)
                    .height(32.0)
                    .background(Color(1.0, 1.0, 1.0, 0.8))
                    .rounded_corners(16.0),
                BoxSpec::new(),
                || {},
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn CardListScene(list_state: LazyListState) {
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.85, 0.86, 0.88, 1.0)),
        BoxSpec::new(),
        move || {
            LazyColumn(
                Modifier::empty().fill_max_size().padding(32.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(28.0)),
                move |scope| {
                    scope.items(LazyItems::new(400).key(|i: usize| i as u64), CardRow);
                },
            );
            // The fixed glass chrome cranscan holds over its Library scroll:
            // a toolbar and a tab bar, each an uncacheable backdrop that
            // re-captures and re-blurs every scrolled frame. PROFILE_BARS
            // (default 2) sizes the chrome so the per-backdrop-boundary cost
            // can be measured by delta.
            let bars: usize = std::env::var("PROFILE_BARS")
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(2);
            if bars >= 1 {
                GlassBar(24.0, 40.0, FRAME_WIDTH as f32 - 48.0, 200.0);
            }
            if bars >= 2 {
                GlassBar(
                    24.0,
                    FRAME_HEIGHT as f32 - 240.0,
                    FRAME_WIDTH as f32 - 48.0,
                    200.0,
                );
            }
        },
    );
}

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("adapter");
    eprintln!(
        "adapter: {} ({:?}) timestamps={}",
        adapter.get_info().name,
        adapter.get_info().backend,
        adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY),
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Pass Timing Profile Device"),
        required_features: cranpose_render_wgpu::optional_device_features(&adapter),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("device");

    let mut renderer = cranpose_render_wgpu::WgpuRenderer::new(&[
        cranpose_render_common::software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES,
    ]);
    renderer.init_gpu(
        std::sync::Arc::new(device),
        std::sync::Arc::new(queue),
        wgpu::TextureFormat::Bgra8UnormSrgb,
        adapter.get_info().backend,
        adapter.get_downlevel_capabilities().flags,
    );

    let root_key = location_key(file!(), line!(), column!());
    let list_state: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let list_state_for_app = Rc::clone(&list_state);
    let mut shell = AppShell::new(renderer, root_key, move || {
        let state = rememberLazyListState();
        *list_state_for_app.borrow_mut() = Some(state);
        CardListScene(state);
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();

    let scroll = |shell: &mut AppShell<cranpose_render_wgpu::WgpuRenderer>| {
        let state = list_state
            .borrow()
            .as_ref()
            .cloned()
            .expect("list state captured");
        shell.debug_enter_app_context(|| state.dispatch_scroll_delta(SCROLL_DELTA_PER_FRAME));
    };

    for _ in 0..WARMUP_FRAMES {
        scroll(&mut shell);
        shell.update();
        shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture");
    }
    // A fresh window after warmup: everything cached that will ever cache.
    let start = std::time::Instant::now();
    for _ in 0..MEASURED_FRAMES {
        scroll(&mut shell);
        shell.update();
        shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture");
    }
    let elapsed = start.elapsed().as_secs_f64();

    let report = shell.renderer().gpu_pass_timings();
    let stats = shell.renderer().last_frame_stats().expect("frame stats");
    println!(
        "frames={} wall_fps={:.1} report_frames={}",
        MEASURED_FRAMES,
        MEASURED_FRAMES as f64 / elapsed,
        report.frames,
    );
    println!(
        "counters: passes={} blur={} composite={} shadow_hit_px={:.2}MP layer_miss_px={:.2}MP",
        stats.pass_count,
        stats.blur_passes,
        stats.composite_passes,
        stats.shadow_shape_cache_hit_pixels as f64 / 1_000_000.0,
        stats.layer_cache_miss_pixels as f64 / 1_000_000.0,
    );
    let frames = f64::from(report.frames.max(1));
    let total_ms: f64 = report.entries.iter().map(|entry| entry.total_ms).sum();
    println!(
        "gpu_span={:.3}ms/frame occupancy={:.3}ms/frame",
        report.span_ms / frames,
        total_ms / frames,
    );
    for entry in &report.entries {
        println!(
            "  {:<40} {:>8.3}ms/frame  x{:>5.1}",
            entry.label,
            entry.total_ms / frames,
            entry.passes as f64 / frames,
        );
    }
}
