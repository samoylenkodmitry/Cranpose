#![allow(dead_code)]

use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard, mpsc},
    time::Duration,
};

use cranpose_app_shell::AppShell;
use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, RenderGraph, RenderNode},
    software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES,
};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    AppContext, Color, Modifier, Size, composable,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::Rect;

pub static TEST_FONT: &[u8] = DEFAULT_SOFTWARE_TEXT_FONT_BYTES;

pub fn layer_node(
    node_id: Option<NodeId>,
    width: f32,
    height: f32,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id,
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        children,
        ..Default::default()
    }
}

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn gpu_test_lock() -> MutexGuard<'static, ()> {
    lock_gpu_test()
}

struct StderrWarnings;

impl log::Log for StderrWarnings {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static STDERR_WARNINGS: StderrWarnings = StderrWarnings;

fn lock_gpu_test() -> MutexGuard<'static, ()> {
    if log::set_logger(&STDERR_WARNINGS).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    GPU_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct LockedRenderer {
    _lock: MutexGuard<'static, ()>,
    app_context: std::rc::Rc<AppContext>,
    renderer: WgpuRenderer,
}

impl Deref for LockedRenderer {
    type Target = WgpuRenderer;

    fn deref(&self) -> &Self::Target {
        &self.renderer
    }
}

impl DerefMut for LockedRenderer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.renderer
    }
}

impl LockedRenderer {
    pub fn render_current_scene_to_texture(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<RenderStatsSnapshot, String> {
        self.app_context.enter(|| {
            let device = self
                .renderer
                .try_device()
                .ok_or_else(|| "renderer GPU device was not initialized".to_string())?;
            let (texture, view) = render_target(
                device,
                width,
                height,
                wgpu::TextureFormat::Bgra8UnormSrgb,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            self.renderer
                .render(&texture, &view, width, height)
                .map_err(|err| format!("{err:?}"))?;
            self.renderer
                .last_frame_stats()
                .ok_or_else(|| "renderer did not publish frame stats".to_string())
        })
    }

    pub fn capture_frame(&mut self, width: u32, height: u32) -> Result<CapturedFrame, String> {
        self.app_context.enter(|| {
            self.renderer
                .capture_frame(width, height)
                .map_err(|err| format!("{err:?}"))
        })
    }

    pub fn capture_frame_with_scale(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<CapturedFrame, String> {
        self.app_context.enter(|| {
            self.renderer
                .capture_frame_with_scale(width, height, root_scale)
                .map_err(|err| format!("{err:?}"))
        })
    }

    pub fn last_frame_stats(&self) -> Option<RenderStatsSnapshot> {
        self.app_context.enter(|| self.renderer.last_frame_stats())
    }
}

pub fn headless_renderer() -> Result<LockedRenderer, String> {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("0"));
    let lock = lock_gpu_test();
    let mut renderer = create_headless_renderer()?;
    let app_context = AppContext::new();
    renderer.attach_app_context_services(&app_context);
    Ok(LockedRenderer {
        _lock: lock,
        app_context,
        renderer,
    })
}

pub fn headless_renderer_unencoded() -> Result<LockedRenderer, String> {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_SEGMENT_SURFACE", Some("0"));
    let lock = lock_gpu_test();
    let mut renderer = create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8Unorm)?;
    let app_context = AppContext::new();
    renderer.attach_app_context_services(&app_context);
    Ok(LockedRenderer {
        _lock: lock,
        app_context,
        renderer,
    })
}

pub fn headless_renderer_parts() -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer()?;
    Ok((lock, renderer))
}

pub fn headless_renderer_parts_with_display_format(
    format: wgpu::TextureFormat,
) -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(format)?;
    Ok((lock, renderer))
}

pub fn headless_renderer_parts_configured<T>(
    configure: impl FnOnce() -> T,
) -> Result<(MutexGuard<'static, ()>, T, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let configured = configure();
    let renderer = create_headless_renderer()?;
    Ok((lock, configured, renderer))
}

pub fn reinit_gpu(renderer: &mut LockedRenderer) -> Result<(), String> {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Replacement Contract Test Device"),
        required_features: cranpose_render_wgpu::optional_device_features(&adapter),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| format!("device request failed: {err:?}"))?;
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Bgra8UnormSrgb,
        adapter.get_info().backend,
        adapter.get_downlevel_capabilities().flags,
    );
    Ok(())
}

pub fn headless_renderer_parts_unencoded() -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String>
{
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8Unorm)?;
    Ok((lock, renderer))
}

fn create_headless_renderer() -> Result<WgpuRenderer, String> {
    create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8UnormSrgb)
}

fn create_headless_renderer_with_format(
    surface_format: wgpu::TextureFormat,
) -> Result<WgpuRenderer, String> {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Shared Render Contract Test Device"),
        required_features: cranpose_render_wgpu::optional_device_features(&adapter),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| format!("device request failed: {err:?}"))?;

    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        surface_format,
        adapter.get_info().backend,
        adapter.get_downlevel_capabilities().flags,
    );
    Ok(renderer)
}

/// Composes `page` in an app shell over a fresh headless renderer, laid out
/// and updated twice so caches are warm, or `None` when no GPU is available.
/// Everything a composable page test imports: the page widgets and the
/// frame helpers below.
#[allow(unused_imports)]
pub mod page {
    pub use cranpose_ui::{
        Color, Modifier, RenderEffect, TextStyle, composable,
        widgets::{Box, BoxSpec, Text},
    };

    pub use super::{FramePage, rect_modifier};
}

/// Everything a raw render-graph test imports: the graph node types and the
/// drawing primitives that fill a draw run.
#[allow(unused_imports)]
pub mod graph {
    pub use cranpose_render_common::{
        Renderer,
        graph::{RenderGraph, RenderNode},
    };
    pub use cranpose_ui_graphics::{
        Brush, Color, CornerRadii, DrawScope, DrawScopeDefault, Point, Rect, Stroke, TileMode,
    };

    pub use super::draw_run_graph;
}

/// A root layer of `size` square pixels holding one draw run of `scope`'s
/// primitives.
pub fn draw_run_graph(size: u32, scope: cranpose_ui_graphics::DrawScopeDefault) -> RenderGraph {
    use cranpose_render_common::graph::{DrawRunNode, PrimitivePhase};
    use cranpose_ui_graphics::DrawScope;
    RenderGraph::new(layer_node(
        None,
        size as f32,
        size as f32,
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            scope.into_primitives(),
        ))],
    ))
}

/// A render-attachment texture of `format` and its default view, the shape
/// every presentable-target test hands to the renderer.
pub fn read_texture_rgba8(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    let width = texture.width();
    let height = texture.height();
    let unpadded = width * 4;
    let padded = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: None,
    });
    rx.recv_timeout(Duration::from_secs(3))
        .expect("readback timed out")
        .expect("readback map failed");
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    pixels
}

pub fn render_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Test Render Target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub fn app_shell_for(
    page: fn(),
    width: u32,
    height: u32,
    display_format: wgpu::TextureFormat,
    configure: impl FnOnce(&mut WgpuRenderer),
) -> Option<(MutexGuard<'static, ()>, AppShell<WgpuRenderer>)> {
    let (lock, mut renderer) = match headless_renderer_parts_with_display_format(display_format) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };
    configure(&mut renderer);
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, page);
    shell.set_viewport(width as f32, height as f32);
    shell.set_buffer_size(width, height);
    shell.update();
    shell.update();
    Some((lock, shell))
}

/// Captures `graph` three times and returns the last frame, after checking
/// that repeated captures of one graph are byte-stable.
pub fn stable_capture(renderer: &mut LockedRenderer, graph: &RenderGraph, size: u32) -> Vec<u8> {
    let mut passes = Vec::new();
    for _ in 0..3 {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(size, size)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (size, size));
        passes.push(captured.pixels);
    }
    assert_eq!(
        passes[1], passes[2],
        "same-graph control passes must be byte-stable before the cross-arm compare"
    );
    passes.pop().expect("three captures")
}

pub fn distinct_colors(pixels: &[u8]) -> usize {
    let mut colors: Vec<[u8; 4]> = pixels.as_chunks::<4>().0.to_vec();
    colors.sort_unstable();
    colors.dedup();
    colors.len()
}

/// Asserts two RGBA frames of `width` pixels per row are identical, naming
/// the first differing pixel otherwise.
pub fn assert_same_bytes(label: &str, width: u32, a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len(), "{label}: capture sizes differ");
    let mut differing = 0usize;
    let mut worst = 0u8;
    let mut first = None;
    for (index, (x, y)) in a.iter().zip(b).enumerate() {
        let diff = x.abs_diff(*y);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
            first.get_or_insert((index / 4 % width as usize, index / 4 / width as usize));
        }
    }
    assert_eq!(
        differing, 0,
        "{label}: {differing} bytes diverged (worst {worst}, first at {first:?})"
    );
}

pub fn rect_modifier(rect: [f32; 4]) -> Modifier {
    Modifier::empty().offset(rect[0], rect[1]).size(Size {
        width: rect[2],
        height: rect[3],
    })
}

/// A page filling the whole frame with one background color, the root every
/// parity scene composes its content into.
#[composable]
#[allow(non_snake_case)]
pub fn FramePage(width: u32, height: u32, background: Color, content: impl Fn() + 'static) {
    Box(
        Modifier::empty()
            .size(Size {
                width: width as f32,
                height: height as f32,
            })
            .background(background),
        BoxSpec::new(),
        content,
    );
}
