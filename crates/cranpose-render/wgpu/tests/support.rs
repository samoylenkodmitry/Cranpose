#![allow(dead_code)]

#[path = "support/device.rs"]
mod device;

use std::{
    ops::{Deref, DerefMut},
    sync::{Mutex, MutexGuard, mpsc},
    time::Duration,
};

use cranpose_app_shell::AppShell;
use cranpose_core::NodeId;
use cranpose_liquid::{GlassDeformation, GlassDynamics, GlassMorph};
use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawCommandId, DrawPrimitiveNode, DrawRunNode, LayerNode, PrimitiveEntry,
        PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES,
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::{CapturedFrame, PresentOutcome, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    AppContext, Color, Modifier, Size, composable,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, DrawPrimitive, DrawScope, DrawScopeDefault, LiquidGlassRect, LiquidGlassSpec, Point,
    RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader, SubstrateSpec,
    liquid_glass_effect,
};

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
    renderer: WgpuRenderer,
    app_context: std::rc::Rc<AppContext>,
    _lock: MutexGuard<'static, ()>,
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
    headless_renderer_with_limits(wgpu::Limits::default())
}

pub fn headless_renderer_with_limits(limits: wgpu::Limits) -> Result<LockedRenderer, String> {
    headless_renderer_configured(limits, wgpu::Backends::all())
}

pub fn headless_renderer_configured(
    limits: wgpu::Limits,
    backends: wgpu::Backends,
) -> Result<LockedRenderer, String> {
    let lock = lock_gpu_test();
    let mut renderer =
        create_headless_renderer_configured(wgpu::TextureFormat::Bgra8UnormSrgb, limits, backends)?;
    let app_context = AppContext::new();
    renderer.attach_app_context_services(&app_context);
    Ok(LockedRenderer {
        _lock: lock,
        app_context,
        renderer,
    })
}

pub fn headless_renderer_unencoded() -> Result<LockedRenderer, String> {
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
    device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Replacement Contract Test Device",
    )?
    .attach(renderer, wgpu::TextureFormat::Bgra8UnormSrgb);
    Ok(())
}

pub fn headless_renderer_parts_unencoded() -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String>
{
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8Unorm)?;
    Ok((lock, renderer))
}

/// A second renderer beside one already holding the GPU test lock.
pub fn headless_renderer_beside_locked() -> Result<WgpuRenderer, String> {
    create_headless_renderer()
}

fn create_headless_renderer() -> Result<WgpuRenderer, String> {
    create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8UnormSrgb)
}

fn create_headless_renderer_with_format(
    surface_format: wgpu::TextureFormat,
) -> Result<WgpuRenderer, String> {
    create_headless_renderer_with_format_and_limits(surface_format, wgpu::Limits::default())
}

fn create_headless_renderer_with_format_and_limits(
    surface_format: wgpu::TextureFormat,
    limits: wgpu::Limits,
) -> Result<WgpuRenderer, String> {
    create_headless_renderer_configured(surface_format, limits, wgpu::Backends::all())
}

fn create_headless_renderer_configured(
    surface_format: wgpu::TextureFormat,
    limits: wgpu::Limits,
    backends: wgpu::Backends,
) -> Result<WgpuRenderer, String> {
    let device =
        device::HeadlessDevice::request(backends, limits, "Shared Render Contract Test Device")?;
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    device.attach(&mut renderer, surface_format);
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

/// The dynamics of a lens morphing inside a node: one primary shape with
/// wobble, bulge, ellipse blend and an incompressible strain along x.
pub fn morphing_lens_dynamics(node: Rect, primary: (f32, f32, f32, f32, f32)) -> GlassDynamics {
    GlassDynamics {
        activity: Some(1.0),
        morph: Some(GlassMorph {
            node_size: (node.width, node.height),
            primary,
            shapes: Vec::new(),
            glue: 0.0,
            wobble_amplitude: 1.0,
            wobble_phase: 0.4,
            bulge_amplitude: 2.0,
            bulge_direction: 0.7,
            ellipse_blend: 0.5,
            deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.05)),
            zoom_anchor: (0.0, 0.0),
        }),
        ..Default::default()
    }
}

/// Composes `page` in an app shell, captures it twice and returns the second
/// frame with the stats it recorded; `None` when headless WGPU is unavailable.
pub fn warm_app_frame(
    page: fn(),
    width: u32,
    height: u32,
) -> Option<(CapturedFrame, RenderStatsSnapshot)> {
    let (_lock, mut shell) = app_shell_for(
        page,
        width,
        height,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )?;
    shell
        .renderer()
        .capture_frame(width, height)
        .expect("warm-up capture should succeed");
    let frame = shell
        .renderer()
        .capture_frame(width, height)
        .expect("frame capture should succeed");
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    let stats = shell
        .renderer()
        .last_frame_stats()
        .expect("the capture recorded frame stats");
    Some((frame, stats))
}

/// The pixels of two RGBA8 frames of `width` that differ: `(x, y, a, b)`.
pub fn differing_pixels(width: u32, a: &[u8], b: &[u8]) -> Vec<(usize, usize, [u8; 4], [u8; 4])> {
    assert_eq!(a.len(), b.len(), "frames of different sizes");
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, (a, b))| (index % width as usize, index / width as usize, *a, *b))
        .collect()
}

/// A one-line account of `differing`: the count, the bounding box and the
/// first few pixels.
pub fn describe_differing(differing: &[(usize, usize, [u8; 4], [u8; 4])]) -> String {
    let bbox = differing
        .iter()
        .fold((usize::MAX, usize::MAX, 0, 0), |b, (x, y, _, _)| {
            (b.0.min(*x), b.1.min(*y), b.2.max(*x), b.3.max(*y))
        });
    format!(
        "{} pixels; bbox x {}..={} y {}..={}; first {:?}",
        differing.len(),
        bbox.0,
        bbox.2,
        bbox.1,
        bbox.3,
        &differing[..differing.len().min(6)]
    )
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
/// Vertical stripes in three hues crossed by horizontal bands over a dark
/// page of `width` x `height`, so every tap of a kernel meets an edge
/// somewhere: a blur at the wrong radius, a tap in the wrong texels or a
/// neighbour's texels all change pixels.
pub fn striped_page(width: u32, height: u32) -> Vec<RenderNode> {
    let mut nodes = vec![solid_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
        Color::from_rgb_u8(24, 28, 40),
    )];
    for index in 0..width.div_ceil(8) {
        let color = match index % 3 {
            0 => Color::from_rgb_u8(230, 90, 60),
            1 => Color::from_rgb_u8(70, 200, 120),
            _ => Color::from_rgb_u8(80, 110, 240),
        };
        nodes.push(solid_rect(
            Rect {
                x: index as f32 * 8.0,
                y: 0.0,
                width: 4.0,
                height: height as f32,
            },
            color,
        ));
    }
    for index in 0..height.div_ceil(10) {
        nodes.push(solid_rect(
            Rect {
                x: 0.0,
                y: index as f32 * 10.0 + 3.0,
                width: width as f32,
                height: 3.0,
            },
            Color::from_rgb_u8(245, 225, 90),
        ));
    }
    nodes
}
/// How a substrate probe reads its substrate: the texel of the block its
/// pixel lies in, or a bilinear read held to the substrate's texel centers.
#[derive(Clone, Copy)]
pub enum SubstrateProbeRead {
    BlockTexel,
    Held,
}

/// A batched shader declaring `spec` that paints its first substrate at its
/// own coordinate, magenta when the renderer packed none.
pub fn substrate_probe(spec: SubstrateSpec, read: SubstrateProbeRead) -> RenderEffect {
    let uv = match read {
        SubstrateProbeRead::BlockTexel => {
            "(substrate.xy + floor(input.uv * substrate.zw) + vec2<f32>(0.5)) / dims"
        }
        SubstrateProbeRead::Held => {
            "(substrate.xy + clamp(input.uv, 0.5 / substrate.zw, vec2<f32>(1.0) - 0.5 / substrate.zw) * substrate.zw) / dims"
        }
    };
    let mut shader = RuntimeShader::new(&format!(
        "{}\n@fragment\nfn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{\n    let substrate = u[58u];\n    if (substrate.z < 0.5) {{\n        return vec4<f32>(1.0, 0.0, 1.0, 1.0);\n    }}\n    let dims = vec2<f32>(textureDimensions(input_texture));\n    let uv = {uv};\n    return vec4<f32>(textureSampleLevel(input_texture, input_sampler, uv, 0.0).rgb, 1.0);\n}}\n",
        RUNTIME_SHADER_PRELUDE_WGSL,
    ));
    shader.set_batched_source(true);
    shader.set_substrates(vec![spec]);
    RenderEffect::Shader { shader }
}

/// A page of the given size holding `children` in order.
pub fn page_graph(width: u32, height: u32, children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(layer_node(None, width as f32, height as f32, children))
}

/// Renders `graph` through the renderer and captures a frame of the given size.
pub fn capture_graph(
    renderer: &mut LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
) -> CapturedFrame {
    capture_graph_with_scale(renderer, graph, width, height, 1.0)
}

/// Renders `graph` with its root scaled by `root_scale` and captures a frame
/// of the given size.
pub fn capture_graph_with_scale(
    renderer: &mut LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
    root_scale: f32,
) -> CapturedFrame {
    renderer.scene_mut().graph = Some(graph);
    renderer
        .capture_frame_with_scale(width, height, root_scale)
        .expect("capture should succeed")
}

/// The RGBA bytes of `region` of the frame, row by row.
pub fn region_pixels(frame: &CapturedFrame, region: Rect) -> Vec<u8> {
    let left = region.x as u32;
    let top = region.y as u32;
    let width = region.width as u32;
    let height = region.height as u32;
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for y in top..top + height {
        let start = ((y * frame.width + left) * 4) as usize;
        out.extend_from_slice(&frame.pixels[start..start + (width * 4) as usize]);
    }
    out
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

/// One solid-filled rectangle drawn before the children.
pub fn solid_rect(rect: Rect, color: Color) -> RenderNode {
    brush_rect(rect, Brush::solid(color))
}

pub fn brush_rect(rect: Rect, brush: Brush) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect,
                brush,
                stroke: None,
            },
            clip: None,
        }),
    })
}

pub const SIZE: u32 = 256;
pub const CENTER: f32 = 128.0;

/// A scene of solid arcs, discs, rounded rects and strokes, the shape of
/// cranorbit's arena.
pub fn record_solid_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    for ring in 0..3u32 {
        let radius = 40.0 + ring as f32 * 28.0;
        let count = 48;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + ring as f32 * 0.11;
            scope.draw_annular_sector(
                Brush::solid(Color(0.3, 0.5 + (i % 5) as f32 * 0.08, 0.8, 1.0)),
                Point::new(CENTER, CENTER),
                radius - 6.0,
                radius,
                start,
                0.09,
            );
        }
    }
    for m in 0..7u32 {
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.9)),
            Point::new(24.0 + m as f32 * 32.0, 20.0),
            5.5,
        );
    }
    scope.draw_round_rect_at(
        Rect {
            x: 30.0,
            y: 220.0,
            width: 80.0,
            height: 24.0,
        },
        Brush::solid(Color(0.8, 0.3, 0.4, 1.0)),
        cranpose_ui_graphics::CornerRadii::uniform(8.0),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 140.0,
            y: 218.0,
            width: 84.0,
            height: 28.0,
        },
        Brush::solid(Color(0.4, 0.9, 0.6, 1.0)),
        cranpose_ui_graphics::Stroke {
            width: 3.0,
            ..Default::default()
        },
    );
}

/// [`record_solid_scene`] plus linear and radial gradient fills and a
/// gradient stroke.
pub fn record_mixed_scene(scope: &mut DrawScopeDefault) {
    record_solid_scene(scope);
    scope.draw_rect_at(
        Rect {
            x: 4.0,
            y: 120.0,
            width: 8.0,
            height: 8.0,
        },
        Brush::linear_gradient(vec![Color(1.0, 0.0, 0.0, 0.0), Color(0.0, 1.0, 0.0, 0.0)]),
    );
    scope.draw_rect_at(
        Rect {
            x: 150.0,
            y: 40.0,
            width: 90.0,
            height: 60.0,
        },
        Brush::radial_gradient(
            vec![Color(0.9, 0.2, 0.2, 1.0), Color(0.1, 0.1, 0.6, 0.2)],
            Point::new(195.0, 70.0),
            50.0,
        ),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 60.0,
            y: 150.0,
            width: 120.0,
            height: 50.0,
        },
        Brush::linear_gradient(vec![Color(0.2, 0.9, 0.3, 1.0), Color(0.9, 0.9, 0.1, 1.0)]),
        cranpose_ui_graphics::Stroke {
            width: 4.0,
            ..Default::default()
        },
    );
}

/// A layer node with every field spelled out, for contract tests that
/// build graphs by hand.
pub fn contract_layer(
    node_id: Option<NodeId>,
    cache_policy: cranpose_render_common::graph::CachePolicy,
    local_bounds: Rect,
    transform_to_parent: cranpose_render_common::graph::ProjectiveTransform,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id,
        wraps: None,
        local_bounds,
        transform_to_parent,
        motion_context_animated: false,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        content_offset: Point::default(),
        scene_children_origin: Point::default(),
        scene_children_layer_translation: Point::default(),
        graphics_layer: cranpose_ui_graphics::GraphicsLayer::default(),
        clip_to_bounds: false,
        shadow_clip: None,
        hit_test: None,
        has_hit_targets: false,
        has_origin_sinks: false,
        isolation: cranpose_render_common::graph::IsolationReasons::default(),
        cache_policy,
        cache_hashes: cranpose_render_common::raster_cache::LayerRasterCacheHashes::default(),
        cache_hashes_valid: false,
        children,
    }
}

/// A solid rect primitive node.
pub fn rect_primitive(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        }),
    })
}

/// The node id of the one command of [`stored_run_graph`].
pub const STORED_RUN_NODE: NodeId = 7_200;

/// The rect the record at `index` of [`stored_run_graph`] draws: eight
/// per row, apart from each other and from the edges.
pub fn stored_run_rect(index: usize) -> Rect {
    Rect {
        x: (index % 8) as f32 * 14.0 + 4.0,
        y: (index / 8) as f32 * 9.0 + 4.0,
        width: 10.0,
        height: 6.0,
    }
}

/// A `width` by `height` graph whose one command records one rect per
/// colour of `colors`: enough records for the run store to retain the
/// command's tables when there are as many as a stored run needs.
pub fn stored_run_graph(width: u32, height: u32, colors: &[Color]) -> RenderGraph {
    let primitives = colors
        .iter()
        .enumerate()
        .map(|(index, color)| DrawPrimitive::Rect {
            rect: stored_run_rect(index),
            brush: Brush::solid(*color),
            stroke: None,
        })
        .collect();
    RenderGraph::new(contract_layer(
        Some(STORED_RUN_NODE),
        CachePolicy::None,
        Rect {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
        },
        ProjectiveTransform::identity(),
        vec![RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id: STORED_RUN_NODE,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            primitives,
        ))],
    ))
}

/// Presents `graph` at `width` by `height` through the renderer's own
/// packet path and reads the pixels back as RGBA8 rows.
pub fn present_and_read(
    renderer: &mut LockedRenderer,
    width: u32,
    height: u32,
    graph: RenderGraph,
) -> Vec<u8> {
    renderer.scene_mut().graph = Some(graph);
    let packet = renderer
        .build_frame_packet_for_tests(width, height)
        .expect("the graph must lower into a packet");
    let (texture, view) = render_target(
        renderer.try_device().expect("device"),
        width,
        height,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let outcome = renderer
        .render_held_packet_for_tests(&texture, &view, width, height, packet)
        .expect("the packet must draw");
    assert_eq!(outcome, PresentOutcome::Presented);
    let device = renderer.try_device().expect("device");
    let queue = renderer.try_queue_for_tests().expect("queue");
    read_texture_rgba8(device, queue, &texture)
}

/// What a reference blur reads past the image: the edge pixel again, or
/// nothing.
#[derive(Clone, Copy)]
pub enum ReferenceEdge {
    Clamp,
    Transparent,
}

/// The renderer's kernel: `ceil(radius)` taps each side, `sigma = radius /
/// 2`, truncated there and normalized.
pub fn reference_kernel(radius: f32) -> Vec<f32> {
    let taps = radius.ceil() as i32;
    let sigma = radius * 0.5;
    let weights: Vec<f32> = (-taps..=taps)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

/// A separable blur of an image of `channels` floats per pixel, horizontal
/// then vertical, reading `edge` past the image.
pub fn reference_blur(
    image: &[f32],
    width: usize,
    height: usize,
    channels: usize,
    radius: f32,
    edge: ReferenceEdge,
) -> Vec<f32> {
    let kernel = reference_kernel(radius);
    let taps = kernel.len() as i32 / 2;
    let sample = |image: &[f32], x: i32, y: i32, c: usize| {
        let (x, y) = match edge {
            ReferenceEdge::Clamp => (x.clamp(0, width as i32 - 1), y.clamp(0, height as i32 - 1)),
            ReferenceEdge::Transparent => {
                if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                    return 0.0;
                }
                (x, y)
            }
        };
        image[(y as usize * width + x as usize) * channels + c]
    };
    let pass = |image: &[f32], (dx, dy): (i32, i32)| {
        let mut out = vec![0.0f32; image.len()];
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                for c in 0..channels {
                    out[(y as usize * width + x as usize) * channels + c] = kernel
                        .iter()
                        .enumerate()
                        .map(|(k, weight)| {
                            let offset = k as i32 - taps;
                            weight * sample(image, x + dx * offset, y + dy * offset, c)
                        })
                        .sum();
                }
            }
        }
        out
    };
    pass(&pass(image, (1, 0)), (0, 1))
}

pub fn max_channel_delta(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "pixel buffer lengths");
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

pub mod glass_page {
    use super::*;

    pub const FRAME_WIDTH: u32 = 240;
    pub const FRAME_HEIGHT: u32 = 120;
    pub const GLASS_WIDTH: f32 = 56.0;
    pub const GLASS_HEIGHT: f32 = 40.0;
    pub const GLASS_TOP: f32 = 30.0;
    pub const GLASS_PITCH: f32 = 72.0;
    pub const GLASS_LEFT: f32 = 12.0;
    pub const GLASS_RADIUS: f32 = 12.0;
    pub const BLUR_RADIUS: f32 = 6.0;

    pub fn glass_shader() -> RenderEffect {
        liquid_glass_effect(
            &LiquidGlassRect {
                left: 0.0,
                top: 0.0,
                width: GLASS_WIDTH,
                height: GLASS_HEIGHT,
                tint_color: Color(1.0, 1.0, 1.0, 0.12),
            },
            &LiquidGlassSpec::default(),
            GLASS_WIDTH,
            GLASS_HEIGHT,
        )
    }
}
