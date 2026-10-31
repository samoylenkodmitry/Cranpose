#![expect(dead_code, reason = "each test binary uses a part of the support")]

use std::sync::PoisonError;

use cranpose_ui_graphics::DrawPlacement;
#[path = "support/device.rs"]
pub(crate) mod device;

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
    text_fonts::DEFAULT_FONT_BYTES,
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

pub(crate) static TEST_FONT: &[u8] = DEFAULT_FONT_BYTES;

pub(crate) use crate::shared_test_support::{draw_text_layout, prepared_text, with_layout};

pub(crate) fn layer_node(
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

pub(crate) fn gpu_test_lock() -> MutexGuard<'static, ()> {
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

/// Takes the GPU for one test, with per-material glass folds on.
///
/// The folds ship on Android and nowhere else, and the parity suites are
/// what hold them byte-identical to the plain shader, so every renderer
/// test runs them whatever the platform. A test of the plain path turns
/// them off itself, under this lock, and puts them back.
fn lock_gpu_test() -> MutexGuard<'static, ()> {
    if log::set_logger(&STDERR_WARNINGS).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
    let lock = GPU_TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    cranpose_ui_graphics::set_glass_material_folds(true);
    lock
}

pub(crate) struct LockedRenderer {
    renderer: WgpuRenderer,
    app_context: std::rc::Rc<AppContext>,
    _lock: Option<MutexGuard<'static, ()>>,
}

fn with_app_context(
    mut renderer: WgpuRenderer,
    lock: Option<MutexGuard<'static, ()>>,
) -> LockedRenderer {
    let app_context = AppContext::new();
    renderer.attach_app_context_services(&app_context);
    LockedRenderer {
        renderer,
        app_context,
        _lock: lock,
    }
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
    pub(crate) fn beside_locked() -> Result<Self, String> {
        Ok(with_app_context(headless_renderer_beside_locked()?, None))
    }

    pub(crate) fn compiling_in_background_beside_locked() -> Result<Self, String> {
        Ok(with_app_context(
            create_background_compiling_renderer()?,
            None,
        ))
    }

    pub(crate) fn render_current_scene_to_texture(
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

    pub(crate) fn display_into(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) -> Result<(RenderStatsSnapshot, Vec<u8>), String> {
        self.app_context.enter(|| {
            self.renderer
                .render(texture, view, texture.width(), texture.height())
                .map_err(|err| format!("{err:?}"))?;
            let stats = self
                .renderer
                .last_frame_stats()
                .ok_or_else(|| "renderer did not publish frame stats".to_string())?;
            let device = self.renderer.try_device().ok_or("no device")?;
            let queue = self.renderer.try_queue_for_tests().ok_or("no queue")?;
            Ok((stats, read_texture(device, queue, texture)))
        })
    }

    pub(crate) fn capture_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<CapturedFrame, String> {
        self.app_context.enter(|| {
            self.renderer
                .capture_frame(width, height)
                .map_err(|err| format!("{err:?}"))
        })
    }

    pub(crate) fn capture_frame_with_scale(
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

    pub(crate) fn last_frame_stats(&self) -> Option<RenderStatsSnapshot> {
        self.app_context.enter(|| self.renderer.last_frame_stats())
    }
}

pub(crate) fn headless_renderer() -> Result<LockedRenderer, String> {
    headless_renderer_with_limits(wgpu::Limits::default())
}

pub(crate) fn headless_renderer_compiling_in_background() -> Result<LockedRenderer, String> {
    let lock = lock_gpu_test();
    Ok(with_app_context(
        create_background_compiling_renderer()?,
        Some(lock),
    ))
}

pub(crate) fn headless_renderer_parts_compiling_in_background()
-> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    Ok((lock, create_background_compiling_renderer()?))
}

pub(crate) fn headless_renderer_with_display_format(
    format: wgpu::TextureFormat,
) -> Result<LockedRenderer, String> {
    let (lock, renderer) = headless_renderer_parts_with_display_format(format)?;
    Ok(with_app_context(renderer, Some(lock)))
}

pub(crate) fn headless_renderer_with_limits(
    limits: wgpu::Limits,
) -> Result<LockedRenderer, String> {
    headless_renderer_configured(limits, wgpu::Backends::all())
}

pub(crate) fn headless_renderer_configured(
    limits: wgpu::Limits,
    backends: wgpu::Backends,
) -> Result<LockedRenderer, String> {
    let lock = lock_gpu_test();
    let renderer =
        create_headless_renderer_configured(wgpu::TextureFormat::Bgra8UnormSrgb, limits, backends)?;
    Ok(with_app_context(renderer, Some(lock)))
}

pub(crate) fn renders_copy_free() -> bool {
    device::headless_adapter(wgpu::Backends::all()).is_ok_and(|adapter| {
        adapter.get_info().backend == wgpu::Backend::Metal
            && cranpose_render_wgpu::optional_device_features(&adapter)
                .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
    })
}

pub(crate) fn headless_renderer_copying_uploads() -> Result<LockedRenderer, String> {
    headless_renderer_on(copying_upload_device)
}

pub(crate) fn headless_renderer_parts_copying_uploads()
-> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    Ok((lock, inline_renderer(copying_upload_device()?)))
}

fn copying_upload_device() -> Result<device::HeadlessDevice, String> {
    device::HeadlessDevice::request_without_features(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Copied Upload Test Device",
        wgpu::Features::MAPPABLE_PRIMARY_BUFFERS,
    )
}

pub(crate) fn headless_renderer_without(
    flags: wgpu::DownlevelFlags,
) -> Result<LockedRenderer, String> {
    headless_renderer_on(|| {
        Ok(device::HeadlessDevice::request(
            wgpu::Backends::all(),
            wgpu::Limits::default(),
            "Downlevel Path Test Device",
        )?
        .without(flags))
    })
}

/// A renderer compiling inline on the device `request` makes under the GPU
/// test lock.
fn headless_renderer_on(
    request: impl FnOnce() -> Result<device::HeadlessDevice, String>,
) -> Result<LockedRenderer, String> {
    let lock = lock_gpu_test();
    let renderer = inline_renderer(request()?);
    Ok(with_app_context(renderer, Some(lock)))
}

/// A renderer on `device` compiling every pipeline where first needed.
fn inline_renderer(device: device::HeadlessDevice) -> WgpuRenderer {
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    device.attach(
        &mut renderer,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        device::Pipelines::Inline,
    );
    renderer
}

pub(crate) fn headless_renderer_unencoded() -> Result<LockedRenderer, String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8Unorm)?;
    Ok(with_app_context(renderer, Some(lock)))
}

pub(crate) fn headless_renderer_parts() -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer()?;
    Ok((lock, renderer))
}

pub(crate) fn headless_renderer_parts_with_display_format(
    format: wgpu::TextureFormat,
) -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(format)?;
    Ok((lock, renderer))
}

pub(crate) fn headless_renderer_parts_configured<T>(
    configure: impl FnOnce() -> T,
) -> Result<(MutexGuard<'static, ()>, T, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let configured = configure();
    let renderer = create_background_compiling_renderer()?;
    Ok((lock, configured, renderer))
}

pub(crate) fn reinit_gpu(renderer: &mut LockedRenderer) -> Result<(), String> {
    device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Replacement Contract Test Device",
    )?
    .attach(
        renderer,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        device::Pipelines::Background,
    );
    Ok(())
}

pub(crate) fn headless_renderer_parts_unencoded()
-> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer_with_format(wgpu::TextureFormat::Bgra8Unorm)?;
    Ok((lock, renderer))
}

pub(crate) fn headless_renderer_beside_locked() -> Result<WgpuRenderer, String> {
    let device = device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Reference Render Contract Test Device",
    )?;
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    device.attach(
        &mut renderer,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        device::Pipelines::Inline,
    );
    Ok(renderer)
}

fn create_background_compiling_renderer() -> Result<WgpuRenderer, String> {
    let device = device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        "Background Compiling Test Device",
    )?;
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    device.attach(
        &mut renderer,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        device::Pipelines::Background,
    );
    Ok(renderer)
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
    device.attach(&mut renderer, surface_format, device::Pipelines::Inline);
    Ok(renderer)
}

pub(crate) mod page {
    pub(crate) use cranpose_ui::{
        Color, Modifier, RenderEffect, TextStyle, composable,
        widgets::{Box, BoxSpec, Text},
    };

    pub(crate) use super::{FramePage, rect_modifier};
}

#[expect(unused_imports, reason = "each test binary uses a part of the support")]
pub(crate) mod graph {
    pub(crate) use cranpose_render_common::{
        Renderer,
        graph::{RenderGraph, RenderNode},
    };
    pub(crate) use cranpose_ui_graphics::{
        Brush, Color, CornerRadii, DrawScope, DrawScopeDefault, Point, Rect, Stroke, TileMode,
    };

    pub(crate) use super::draw_run_graph;
}

pub(crate) fn draw_run_graph(size: u32, scope: DrawScopeDefault<'_>) -> RenderGraph {
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

pub(crate) fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    let width = texture.width();
    let height = texture.height();
    let bytes_per_texel = texture
        .format()
        .block_copy_size(None)
        .expect("an uncompressed colour format");
    let unpadded = width * bytes_per_texel;
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
    let mapped = slice.get_mapped_range().expect("mapped readback");
    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    pixels
}

pub(crate) fn render_target(
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

pub(crate) fn app_shell_for(
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

pub(crate) fn update_and_capture(
    shell: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
) -> (RenderStatsSnapshot, CapturedFrame) {
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(width, height)
        .expect("frame capture should succeed");
    let stats = shell.renderer().last_frame_stats().expect("frame stats");
    (stats, frame)
}

pub(crate) fn morphing_lens_dynamics(
    node: Rect,
    primary: (f32, f32, f32, f32, f32),
) -> GlassDynamics {
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
            capsule_smoothing_dp: 0.0,
            deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.05)),
            zoom_anchor: (0.0, 0.0),
        }),
        ..Default::default()
    }
}

pub(crate) fn warm_app_frame(
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
    Some(warm_shell_frame(&mut shell, width, height))
}

pub(crate) fn warm_shell_frame(
    shell: &mut AppShell<WgpuRenderer>,
    width: u32,
    height: u32,
) -> (CapturedFrame, RenderStatsSnapshot) {
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
    (frame, stats)
}

pub(crate) const WRITE_GOLDENS_ENV: &str = "CRANPOSE_WRITE_GOLDENS";
/// Goldens are captured on Metal; other GPUs round anti-aliased edges a step
/// differently. At a fractional root scale the arena run differs in 0.50% of
/// its bytes on both Intel and NVIDIA Vulkan, each by one step (#859).
const GOLDEN_MAX_SMALL_DIFF_FRACTION: f64 = 0.01;
const GOLDEN_SMALL_DIFF: u8 = 2;

pub(crate) fn write_png(path: &std::path::Path, width: u32, height: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("fixture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(pixels).expect("png data");
}

pub(crate) fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::File::open(path)
        .unwrap_or_else(|err| panic!("missing golden {}: {err}", path.display()));
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().expect("png info");
    let mut buffer = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    buffer.truncate(info.buffer_size());
    (info.width, info.height, buffer)
}

pub(crate) fn check_golden(
    name: &str,
    path: &std::path::Path,
    (width, height): (u32, u32),
    pixels: &[u8],
    what: &str,
) {
    if std::env::var_os(WRITE_GOLDENS_ENV).is_some() {
        write_png(path, width, height, pixels);
        eprintln!("{name}: wrote {}", path.display());
        return;
    }
    let (golden_width, golden_height, golden) = read_png(path);
    assert_eq!(
        (golden_width, golden_height),
        (width, height),
        "{name}: golden size"
    );
    let mut small = 0usize;
    let mut large = 0usize;
    let mut worst = 0u8;
    for (a, b) in pixels.iter().zip(&golden) {
        let diff = a.abs_diff(*b);
        worst = worst.max(diff);
        if diff > GOLDEN_SMALL_DIFF {
            large += 1;
        } else if diff > 0 {
            small += 1;
        }
    }
    let small_fraction = small as f64 / pixels.len() as f64;
    eprintln!("{name}: small {small} ({small_fraction:.4}) large {large} worst {worst}");
    assert!(
        large == 0 && small_fraction <= GOLDEN_MAX_SMALL_DIFF_FRACTION,
        "{name}: {large} bytes differ by more than {GOLDEN_SMALL_DIFF} (worst {worst}) and {small} \
         by less ({small_fraction:.4} of the image, bound {GOLDEN_MAX_SMALL_DIFF_FRACTION}); {what}"
    );
}

pub(crate) fn differing_pixels(
    width: u32,
    a: &[u8],
    b: &[u8],
) -> Vec<(usize, usize, [u8; 4], [u8; 4])> {
    pixels_differing_beyond(width, a, b, 0)
}

pub(crate) fn pixels_differing_beyond(
    width: u32,
    a: &[u8],
    b: &[u8],
    tolerance: u8,
) -> Vec<(usize, usize, [u8; 4], [u8; 4])> {
    assert_eq!(a.len(), b.len(), "frames of different sizes");
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (a, b))| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > tolerance))
        .map(|(index, (a, b))| (index % width as usize, index / width as usize, *a, *b))
        .collect()
}

pub(crate) fn describe_differing(differing: &[(usize, usize, [u8; 4], [u8; 4])]) -> String {
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

pub(crate) fn stable_capture(
    renderer: &mut LockedRenderer,
    graph: &RenderGraph,
    size: u32,
) -> Vec<u8> {
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
pub(crate) fn striped_page(width: u32, height: u32) -> Vec<RenderNode> {
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
#[derive(Clone, Copy)]
pub(crate) enum SubstrateProbeRead {
    BlockTexel,
    Held,
}

pub(crate) fn substrate_probe(spec: SubstrateSpec, read: SubstrateProbeRead) -> RenderEffect {
    let uv = match read {
        SubstrateProbeRead::BlockTexel => {
            "(substrate.xy + floor(input.uv * substrate.zw) + vec2<f32>(0.5)) / dims"
        }
        SubstrateProbeRead::Held => {
            "(substrate.xy + clamp(input.uv, 0.5 / substrate.zw, vec2<f32>(1.0) - 0.5 / substrate.zw) * substrate.zw) / dims"
        }
    };
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{\n    let substrate = u[58u];\n    if (substrate.z < 0.5) {{\n        return vec4<f32>(1.0, 0.0, 1.0, 1.0);\n    }}\n    let dims = vec2<f32>(textureDimensions(input_texture));\n    let uv = {uv};\n    return vec4<f32>(textureSampleLevel(input_texture, input_sampler, uv, 0.0).rgb, 1.0);\n}}\n",
    ));
    shader.set_batched_source(true);
    shader.set_substrates(&[spec]);
    RenderEffect::runtime_shader(shader)
}

pub(crate) fn page_graph(width: u32, height: u32, children: Vec<RenderNode>) -> RenderGraph {
    RenderGraph::new(layer_node(None, width as f32, height as f32, children))
}

pub(crate) fn shader_probe_graph(bounds: Rect, shader: RuntimeShader) -> RenderGraph {
    RenderGraph::new(LayerNode {
        local_bounds: bounds,
        graphics_layer: cranpose_ui_graphics::GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            ..Default::default()
        }
        .into(),
        children: vec![solid_rect(bounds, Color::WHITE)],
        ..Default::default()
    })
}

pub(crate) mod clip_band {
    use cranpose_render_common::layer_transform::layer_transform_to_parent;
    use cranpose_ui_graphics::GraphicsLayer;

    use super::*;

    pub(crate) const FRAME: u32 = 200;
    pub(crate) const CLIP_TOP: usize = 80;
    pub(crate) const CHILD: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 120.0,
    };

    pub(crate) fn page(
        layer: GraphicsLayer,
        placement: Point,
        children: Vec<RenderNode>,
    ) -> RenderGraph {
        let child = LayerNode {
            local_bounds: CHILD,
            transform_to_parent: layer_transform_to_parent(CHILD, placement, &layer),
            graphics_layer: layer.into(),
            children,
            ..Default::default()
        };
        let band = LayerNode {
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: FRAME as f32,
                height: FRAME as f32 - CLIP_TOP as f32,
            },
            transform_to_parent: ProjectiveTransform::translation(0.0, CLIP_TOP as f32),
            graphics_layer: GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            }
            .into(),
            children: vec![RenderNode::Layer(Box::new(child))],
            ..Default::default()
        };
        page_graph(FRAME, FRAME, vec![RenderNode::Layer(Box::new(band))])
    }

    pub(crate) fn red_pixels(pixels: &[u8]) -> (usize, usize) {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, px)| px[0] > 160 && px[1] < 90 && px[2] < 90)
            .fold((0, 0), |(above, inside), (index, _)| {
                if (index / FRAME as usize) < CLIP_TOP {
                    (above + 1, inside)
                } else {
                    (above, inside + 1)
                }
            })
    }
}

pub(crate) fn capture_graph(
    renderer: &mut LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
) -> CapturedFrame {
    capture_graph_with_scale(renderer, graph, width, height, 1.0)
}

pub(crate) fn capture_waits(
    renderer: &mut LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
) -> (RenderStatsSnapshot, u64) {
    let (_, stats, waits) = capture_with_waits(renderer, graph, width, height);
    (stats, waits)
}

pub(crate) fn capture_with_waits(
    renderer: &mut LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
) -> (CapturedFrame, RenderStatsSnapshot, u64) {
    let before = cranpose_render_wgpu::pipelines_created();
    let frame = capture_graph(renderer, graph, width, height);
    let stats = renderer.last_frame_stats().expect("frame statistics");
    let built = cranpose_render_wgpu::pipelines_created() - before;
    let waits = built
        + u64::from(stats.placeholder_draws)
        + u64::from(stats.shape_pipeline_fallback_draws)
        + u64::from(stats.instant_draws);
    (frame, stats, waits)
}

pub(crate) fn capture_drawn(
    renderer: &mut LockedRenderer,
    graph: &RenderGraph,
    width: u32,
    height: u32,
) -> (CapturedFrame, RenderStatsSnapshot) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let frame = capture_graph(renderer, graph.clone(), width, height);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        if stats.placeholder_draws == 0 && stats.instant_draws == 0 {
            return (frame, stats);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the effects' pipelines never landed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn capture_graph_with_scale(
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

pub(crate) fn region_pixels(frame: &CapturedFrame, region: Rect) -> Vec<u8> {
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

pub(crate) fn wait_for_background_compiler_idle() {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut seen = cranpose_render_wgpu::pipelines_created_off_frame();
    let mut quiet_since = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let now = cranpose_render_wgpu::pipelines_created_off_frame();
        if now != seen {
            seen = now;
            quiet_since = std::time::Instant::now();
        } else if quiet_since.elapsed() >= Duration::from_secs(1) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the background compiler never went idle"
        );
    }
}

pub(crate) fn pipelines_settled(stats: &RenderStatsSnapshot) -> bool {
    stats.shape_pipeline_fallback_draws == 0
        && stats.shader_pipeline_fallback_draws == 0
        && stats.placeholder_draws == 0
        && stats.instant_draws == 0
}

pub(crate) fn capture_settled(
    renderer: &mut LockedRenderer,
    mut capture: impl FnMut(&mut LockedRenderer) -> CapturedFrame,
) -> CapturedFrame {
    settle(|| {
        let captured = capture(renderer);
        let stats = renderer.last_frame_stats().expect("frame statistics");
        (stats, captured)
    })
}

pub(crate) fn settle<T>(mut frame: impl FnMut() -> (RenderStatsSnapshot, T)) -> T {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (stats, captured) = frame();
        if pipelines_settled(&stats) {
            return captured;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "specialization did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn capture_graph_settled(
    renderer: &mut LockedRenderer,
    graph: &RenderGraph,
    width: u32,
    height: u32,
) -> CapturedFrame {
    capture_settled(renderer, |renderer| {
        capture_graph(renderer, graph.clone(), width, height)
    })
}

pub(crate) fn settled_capture(renderer: &mut LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    stable_settled_capture(renderer, |renderer| {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        captured
    })
}

pub(crate) fn stable_settled_capture(
    renderer: &mut LockedRenderer,
    mut capture: impl FnMut(&mut LockedRenderer) -> CapturedFrame,
) -> Vec<u8> {
    capture_settled(renderer, &mut capture);
    let previous = capture_settled(renderer, &mut capture).pixels;
    let current = capture_settled(renderer, &mut capture).pixels;
    assert_eq!(
        previous, current,
        "same-graph control passes must be byte-stable before the cross-arm compare"
    );
    current
}

pub(crate) fn distinct_colors(pixels: &[u8]) -> usize {
    let mut colors: Vec<[u8; 4]> = pixels.as_chunks::<4>().0.to_vec();
    colors.sort_unstable();
    colors.dedup();
    colors.len()
}

pub(crate) fn assert_same_bytes(label: &str, width: u32, a: &[u8], b: &[u8]) {
    assert_bytes_within(label, width, a, b, 0);
}

pub(crate) fn assert_bytes_within(label: &str, width: u32, a: &[u8], b: &[u8], tolerance: u8) {
    assert_eq!(a.len(), b.len(), "{label}: capture sizes differ");
    let mut differing = 0usize;
    let mut worst = 0u8;
    let mut first = None;
    for (index, (x, y)) in a.iter().zip(b).enumerate() {
        let diff = x.abs_diff(*y);
        if diff > tolerance {
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

pub(crate) fn difference_outside(width: u32, a: &[u8], b: &[u8], rect: [f32; 4]) -> (usize, u8) {
    let inside = |x: f32, y: f32| {
        (rect[0]..rect[0] + rect[2]).contains(&x) && (rect[1]..rect[1] + rect[3]).contains(&y)
    };
    let mut differing = 0usize;
    let mut max_delta = 0u8;
    for (index, (a, b)) in a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (index % width as usize, index / width as usize);
        if inside(x as f32, y as f32) {
            continue;
        }
        let delta = a
            .iter()
            .zip(b)
            .map(|(p, q)| p.abs_diff(*q))
            .max()
            .unwrap_or(0);
        max_delta = max_delta.max(delta);
        differing += usize::from(delta > 0);
    }
    (differing, max_delta)
}

pub(crate) fn rect_modifier(rect: [f32; 4]) -> Modifier {
    Modifier::empty().offset(rect[0], rect[1]).size(Size {
        width: rect[2],
        height: rect[3],
    })
}

/// A page filling the whole frame with one background color, the root every
/// parity scene composes its content into.
#[composable]
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

pub(crate) fn text_node(node_id: usize, text: &str, rect: Rect, font_size: f32) -> RenderNode {
    let style = cranpose_ui::text::TextStyle::from_span_style(cranpose_ui::text::SpanStyle {
        color: Some(Color::WHITE),
        font_size: cranpose_ui::text::TextUnit::Sp(font_size),
        ..Default::default()
    });
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(with_layout(
            cranpose_render_common::graph::TextPrimitiveNode {
                node_id: NodeId(node_id),
                rect,
                text: cranpose_ui::text::shared_plain_annotated_string(text),
                render_text: cranpose_ui::text::shared_plain_render_string(text),
                text_style: std::sync::Arc::new(style),
                font_size,
                overflow: cranpose_ui::TextLayoutOptions::default().overflow,
                clip: None,
                paint: Default::default(),
                layout: Default::default(),
            },
        ))),
    })
}

pub(crate) fn solid_rect(rect: Rect, color: Color) -> RenderNode {
    brush_rect(rect, Brush::solid(color))
}

pub(crate) fn drop_shadow(rect: Rect, color: Color, blur_radius: f32) -> RenderNode {
    draw_node(
        DrawPrimitive::Shadow(cranpose_ui_graphics::ShadowPrimitive::Drop {
            shape: Box::new(DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            }),
            cutout: None,
            blur_radius,
            blend_mode: cranpose_ui_graphics::BlendMode::SrcOver,
        }),
        None,
    )
}

pub(crate) fn brush_rect(rect: Rect, brush: Brush) -> RenderNode {
    draw_node(
        DrawPrimitive::Rect {
            rect,
            brush,
            stroke: None,
        },
        None,
    )
}

pub(crate) fn draw_node(primitive: DrawPrimitive, clip: Option<Rect>) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(Box::new(DrawPrimitiveNode { primitive, clip })),
    })
}

pub(crate) const SIZE: u32 = 256;
pub(crate) const CENTER: f32 = 128.0;

/// The dark background every parity scene draws first.
fn draw_scene_background(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
}

pub(crate) fn record_solid_fill_scene(scope: &mut DrawScopeDefault) {
    draw_scene_background(scope);
    for index in 0..48u32 {
        let x = (index % 8) as f32 * 31.3 + 3.37;
        let y = (index / 8) as f32 * 41.7 + 2.61;
        let size = 6.0 + (index * 7 % 29) as f32;
        let alpha = if index % 3 == 0 { 1.0 } else { 0.85 };
        let brush = Brush::solid(Color(0.3 + (index % 4) as f32 * 0.15, 0.8, 0.5, alpha));
        if index % 2 == 0 {
            scope.draw_circle(
                brush,
                Point::new(x + size / 2.0, y + size / 2.0),
                size / 2.0,
            );
        } else {
            scope.draw_round_rect_at(
                Rect {
                    x,
                    y,
                    width: size,
                    height: size * 1.3,
                },
                brush,
                cranpose_ui_graphics::CornerRadii::uniform(3.0 + (index % 5) as f32),
            );
        }
    }
}

pub(crate) fn record_solid_scene(scope: &mut DrawScopeDefault) {
    draw_scene_background(scope);
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

pub(crate) fn record_mixed_scene(scope: &mut DrawScopeDefault) {
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

pub(crate) fn record_gradient_fill_scene(scope: &mut DrawScopeDefault) {
    use cranpose_ui_graphics::{CornerRadii, TileMode};
    let tile_modes = [
        TileMode::Clamp,
        TileMode::Repeated,
        TileMode::Mirror,
        TileMode::Decal,
    ];
    let separator = |scope: &mut DrawScopeDefault, y: f32| {
        scope.draw_rect_at(
            Rect {
                x: 118.0,
                y,
                width: 8.0,
                height: 4.0,
            },
            Brush::solid(Color(0.5, 0.5, 0.5, 1.0)),
        );
    };
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::linear_gradient_with_tile_mode(
            vec![
                Color(0.05, 0.05, 0.2, 1.0),
                Color(0.2, 0.05, 0.1, 1.0),
                Color(0.02, 0.15, 0.1, 1.0),
            ],
            Point::new(0.0, 0.0),
            Point::new(SIZE as f32, SIZE as f32 * 0.5),
            TileMode::Clamp,
        ),
    );
    for (row, tile_mode) in tile_modes.into_iter().enumerate() {
        let y = 8.0 + row as f32 * 58.0;
        scope.draw_round_rect_at(
            Rect {
                x: 8.0,
                y,
                width: 110.0,
                height: 50.0,
            },
            Brush::linear_gradient_with_tile_mode(
                vec![
                    Color(1.0, 0.2, 0.1, 1.0),
                    Color(0.1, 0.9, 0.3, 0.9),
                    Color(0.2, 0.3, 1.0, 1.0),
                    Color(0.9, 0.9, 0.1, 0.6),
                ],
                Point::new(20.0, y),
                Point::new(60.0, y + 30.0),
                tile_mode,
            ),
            CornerRadii::uniform(6.0 + row as f32 * 4.0),
        );
    }
    scope.draw_round_rect_at(
        Rect {
            x: 8.0,
            y: 240.0,
            width: 110.0,
            height: 12.0,
        },
        Brush::linear_gradient_stops(
            vec![
                (0.0, Color(0.1, 0.1, 0.1, 1.0)),
                (0.15, Color(0.9, 0.2, 0.2, 1.0)),
                (0.3, Color(0.2, 0.9, 0.2, 1.0)),
                (0.55, Color(0.2, 0.2, 0.9, 1.0)),
                (0.8, Color(0.9, 0.9, 0.2, 1.0)),
                (1.0, Color(0.9, 0.9, 0.9, 1.0)),
            ],
            Point::new(8.0, 240.0),
            Point::new(118.0, 252.0),
            TileMode::Clamp,
        ),
        CornerRadii::uniform(4.0),
    );
    separator(scope, 0.0);
    for (row, tile_mode) in tile_modes.into_iter().enumerate() {
        let y = 8.0 + row as f32 * 58.0;
        scope.draw_round_rect_at(
            Rect {
                x: 126.0,
                y,
                width: 110.0,
                height: 50.0,
            },
            Brush::radial_gradient_tiled(
                vec![
                    Color(0.9, 0.5, 0.1, 1.0),
                    Color(0.1, 0.2, 0.7, 0.8),
                    Color(0.6, 0.1, 0.5, 1.0),
                ],
                Point::new(181.0, y + 25.0),
                22.0,
                tile_mode,
            ),
            CornerRadii::uniform(3.0 + row as f32 * 5.0),
        );
    }
    separator(scope, 236.0);
    scope.draw_rect_at(
        Rect {
            x: 126.0,
            y: 240.0,
            width: 110.0,
            height: 12.0,
        },
        Brush::sweep_gradient(
            vec![
                Color(0.9, 0.1, 0.1, 1.0),
                Color(0.1, 0.9, 0.1, 1.0),
                Color(0.1, 0.1, 0.9, 1.0),
            ],
            Point::new(181.0, 246.0),
        ),
    );
}

pub(crate) fn contract_layer(
    node_id: Option<NodeId>,
    cache_policy: CachePolicy,
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        node_id,
        local_bounds,
        transform_to_parent,
        cache_policy,
        children,
        ..Default::default()
    }
}

pub(crate) const STORED_RUN_NODE: NodeId = NodeId(7_200);

pub(crate) fn stored_run_rect(index: usize) -> Rect {
    Rect {
        x: (index % 8) as f32 * 14.0 + 4.0,
        y: (index / 8) as f32 * 9.0 + 4.0,
        width: 10.0,
        height: 6.0,
    }
}

pub(crate) fn stored_run_graph(width: u32, height: u32, colors: &[Color]) -> RenderGraph {
    stored_run_graph_of(width, height, colors.iter().copied().map(Brush::solid))
}

pub(crate) fn stored_run_graph_of(
    width: u32,
    height: u32,
    brushes: impl IntoIterator<Item = Brush>,
) -> RenderGraph {
    stored_runs_graph_of(width, height, [brushes.into_iter().collect()])
}

pub(crate) fn stored_runs_graph_of<const RUNS: usize>(
    width: u32,
    height: u32,
    runs: [Vec<Brush>; RUNS],
) -> RenderGraph {
    let mut first_record = 0;
    let commands = runs
        .into_iter()
        .enumerate()
        .map(|(command_index, brushes)| {
            let start = first_record;
            first_record += brushes.len();
            let primitives = brushes
                .into_iter()
                .enumerate()
                .map(|(index, brush)| DrawPrimitive::Rect {
                    rect: stored_run_rect(start + index),
                    brush,
                    stroke: None,
                })
                .collect();
            RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(DrawCommandId {
                    node_id: STORED_RUN_NODE,
                    command_index: command_index as u32,
                    placement: DrawPlacement::Behind,
                }),
                primitives,
            ))
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
        commands,
    ))
}

pub(crate) fn present_and_read(
    renderer: &mut LockedRenderer,
    width: u32,
    height: u32,
    graph: RenderGraph,
) -> Vec<u8> {
    let texture = present(renderer, width, height, graph);
    read_presented(renderer, &texture)
}

pub(crate) fn present(
    renderer: &mut LockedRenderer,
    width: u32,
    height: u32,
    graph: RenderGraph,
) -> wgpu::Texture {
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
    texture
}

pub(crate) fn read_presented(renderer: &LockedRenderer, texture: &wgpu::Texture) -> Vec<u8> {
    let device = renderer.try_device().expect("device");
    let queue = renderer.try_queue_for_tests().expect("queue");
    read_texture(device, queue, texture)
}

#[derive(Clone, Copy)]
pub(crate) enum ReferenceEdge {
    Clamp,
    Transparent,
}

pub(crate) fn reference_kernel(radius: f32) -> Vec<f32> {
    let taps = radius.ceil() as i32;
    let sigma = radius * 0.5;
    let weights: Vec<f32> = (-taps..=taps)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

pub(crate) fn reference_blur(
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

pub(crate) fn max_channel_delta(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len(), "pixel buffer lengths");
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

pub(crate) mod glass_page {
    use super::*;

    pub(crate) const FRAME_WIDTH: u32 = 240;
    pub(crate) const FRAME_HEIGHT: u32 = 120;
    pub(crate) const GLASS_WIDTH: f32 = 56.0;
    pub(crate) const GLASS_HEIGHT: f32 = 40.0;
    pub(crate) const GLASS_TOP: f32 = 30.0;
    pub(crate) const GLASS_PITCH: f32 = 72.0;
    pub(crate) const GLASS_LEFT: f32 = 12.0;
    pub(crate) const GLASS_RADIUS: f32 = 12.0;
    pub(crate) const BLUR_RADIUS: f32 = 6.0;

    pub(crate) fn panes_page(effects: impl IntoIterator<Item = RenderEffect>) -> RenderGraph {
        let mut children = striped_page(FRAME_WIDTH, FRAME_HEIGHT);
        for (index, effect) in effects.into_iter().enumerate() {
            children.push(RenderNode::Layer(Box::new(
                crate::shared_test_support::layer_node(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: GLASS_WIDTH,
                        height: GLASS_HEIGHT,
                    },
                    ProjectiveTransform::translation(
                        GLASS_LEFT + GLASS_PITCH * index as f32,
                        GLASS_TOP,
                    ),
                    cranpose_ui_graphics::GraphicsLayer {
                        backdrop_effect: Some(effect),
                        clip: true,
                        shape: cranpose_ui_graphics::LayerShape::Rounded(
                            cranpose_ui_graphics::RoundedCornerShape::uniform(GLASS_RADIUS),
                        ),
                        ..cranpose_ui_graphics::GraphicsLayer::default()
                    },
                    Vec::new(),
                ),
            )));
        }
        page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
    }

    pub(crate) struct GlassFolds(bool);

    impl GlassFolds {
        pub(crate) fn set(folds: bool) -> Self {
            let restore = Self(cranpose_ui_graphics::glass_material_folds_enabled());
            cranpose_ui_graphics::set_glass_material_folds(folds);
            restore
        }
    }

    impl Drop for GlassFolds {
        fn drop(&mut self) {
            cranpose_ui_graphics::set_glass_material_folds(self.0);
        }
    }

    pub(crate) fn glass_shader() -> RenderEffect {
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

pub(crate) mod glass_scene {
    use cranpose_ui_graphics::Rect;

    pub(crate) const FRAME_WIDTH: u32 = 240;
    pub(crate) const FRAME_HEIGHT: u32 = 120;
    pub(crate) const GLASS: Rect = Rect {
        x: 80.0,
        y: 30.0,
        width: 96.0,
        height: 60.0,
    };
}

pub(crate) mod bar_scene {
    use cranpose_ui_graphics::Rect;

    pub(crate) const WIDTH: u32 = 160;
    pub(crate) const HEIGHT: u32 = 96;
    pub(crate) const BAR: Rect = Rect {
        x: 20.0,
        y: 24.0,
        width: 120.0,
        height: 48.0,
    };
}
