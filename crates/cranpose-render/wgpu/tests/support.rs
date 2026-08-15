#![allow(dead_code)]

use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use cranpose_render_common::software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES;
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::AppContext;

pub static TEST_FONT: &[u8] = DEFAULT_SOFTWARE_TEXT_FONT_BYTES;

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn gpu_test_lock() -> MutexGuard<'static, ()> {
    lock_gpu_test()
}

fn lock_gpu_test() -> MutexGuard<'static, ()> {
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
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("WGPU contract render target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.renderer
                .render(&view, width, height)
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
    // The segment-surface cache is default-ON since its watch win, and its
    // in-footprint tolerance would leak into every byte-exact suite built on
    // this helper. Pin it OFF at construction; suites that test the cache
    // set "1" per arm after obtaining the renderer, which overrides this.
    std::env::set_var("CRANPOSE_SEGMENT_SURFACE", "0");
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

pub fn headless_renderer_parts() -> Result<(MutexGuard<'static, ()>, WgpuRenderer), String> {
    let lock = lock_gpu_test();
    let renderer = create_headless_renderer()?;
    Ok((lock, renderer))
}

/// Re-initializes an already-initialized renderer's GPU on a fresh headless
/// device — the same `init_gpu`-over-live-renderer replacement semantics as
/// Android's surface recreation path (`renderer_needs_init`).
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
        required_features: wgpu::Features::empty(),
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

/// A headless renderer whose surface format is `Bgra8Unorm` rather than the
/// shared harness's `Bgra8UnormSrgb`.
///
/// An `*UnormSrgb` target makes the GPU gamma-encode whatever the shader wrote
/// on the way into the texture, so a readback is the encoding of the colour
/// rather than the colour. That is fine for tests that count lit pixels and
/// useless for a test that has to compare a composited channel against an exact
/// value: the encode-then-quantise destroys the very level the comparison is
/// about. This gives back the shader's own bytes.
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
        required_features: wgpu::Features::empty(),
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
