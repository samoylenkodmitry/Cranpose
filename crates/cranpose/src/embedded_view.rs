use std::sync::Arc;

use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};

use crate::{AppSettings, frame_readback::FrameTarget};

const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Failure to initialize, resize or draw an embedded component.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddedViewError {
    /// Dimensions must be nonzero, fit the GPU limit and fit the readback budget.
    #[error("invalid embedded size {width} x {height} at density {density}")]
    InvalidSize {
        /// Requested pixel width.
        width: u32,
        /// Requested pixel height.
        height: u32,
        /// Requested pixels per logical point.
        density: f32,
    },
    /// No offscreen GPU adapter is available.
    #[error("no GPU adapter: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    /// The adapter could not create the device.
    #[error("GPU device: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    /// Cranpose could not render the scene.
    #[error("render: {0}")]
    Render(String),
    /// GPU readback failed.
    #[error("readback: {0}")]
    Readback(String),
}

/// A Cranpose composition owned and driven by another application's view.
///
/// This component starts no application, window or event loop. The host owns
/// lifecycle, input and scheduling through [Self::shell]. All methods run on
/// the creating thread. Each instance has its own composition and app context.
///
/// [Self::draw] renders to a reusable GPU target and reads pixels only when
/// changed. This portable adapter copies pixels; it is not a zero-copy native
/// surface. Hosts with a Rust GPU integration can instead use
/// `AppShell<WgpuRenderer>` to render directly into their texture.
pub struct EmbeddedView {
    shell: AppShell<WgpuRenderer>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    target: FrameTarget,
    dirty: bool,
    visible: bool,
    density: f32,
}

impl EmbeddedView {
    /// Creates an independent component with a physical size and logical density.
    ///
    /// Pixel buffers are limited to 64 MiB. Zero-size views should be suspended
    /// with [Self::set_visible] until their native layout has a usable size.
    pub fn new(
        settings: &AppSettings,
        width: u32,
        height: u32,
        density: f32,
        content: impl FnMut() + 'static,
    ) -> Result<Self, EmbeddedViewError> {
        validate_size(width, height, density, u32::MAX)?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Cranpose embedded view"),
                required_features: cranpose_render_wgpu::optional_device_features(&adapter),
                required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
                memory_hints: crate::gpu_limits::mobile_memory_hints(),
                ..Default::default()
            }))?;
        validate_size(
            width,
            height,
            density,
            device.limits().max_texture_dimension_2d,
        )?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let mut renderer = WgpuRenderer::with_text_system(WgpuTextSystem::from_font_set(
            settings.resolve_font_set(),
        ));
        renderer.set_root_scale(density);
        renderer.init_gpu(
            Arc::clone(&device),
            Arc::clone(&queue),
            FRAME_FORMAT,
            adapter.get_info().backend,
            adapter.get_downlevel_capabilities().flags,
        );
        let shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            content,
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );
        let target = FrameTarget::new(&device, width, height, FRAME_FORMAT);
        Ok(Self {
            shell,
            device,
            queue,
            target,
            dirty: true,
            visible: true,
            density,
        })
    }

    /// Accesses input, scheduling, semantics, text input and the app context.
    ///
    /// Install a frame waker, route pointer events in logical coordinates, and
    /// honor `frame_schedule()` after each draw. Native controls keep their own
    /// touch, focus and accessibility handling.
    pub fn shell(&mut self) -> &mut AppShell<WgpuRenderer> {
        &mut self.shell
    }

    /// Updates physical dimensions and density after native layout changes.
    ///
    /// Unchanged dimensions reuse GPU resources. Invalid dimensions leave the
    /// previous configuration intact.
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        density: f32,
    ) -> Result<(), EmbeddedViewError> {
        validate_size(
            width,
            height,
            density,
            self.device.limits().max_texture_dimension_2d,
        )?;
        let size_changed = self.target.size() != (width, height);
        let density_changed = self.density != density;
        if !size_changed && !density_changed {
            return Ok(());
        }
        if size_changed {
            self.target = FrameTarget::new(&self.device, width, height, FRAME_FORMAT);
        }
        self.shell.renderer().set_root_scale(density);
        self.shell
            .primary()
            .set_viewport(width as f32 / density, height as f32 / density);
        self.shell.set_density(density);
        self.density = density;
        self.shell.set_buffer_size(width, height);
        self.dirty = true;
        Ok(())
    }

    /// Suspends drawing while detached or hidden and cancels any active gesture.
    ///
    /// Becoming visible requests a fresh frame without destroying the composition.
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.shell.notify_app_resumed();
            self.dirty = true;
        } else {
            self.shell.cancel_gesture();
            self.shell.notify_app_paused();
        }
    }

    /// Advances the component and writes tightly packed premultiplied RGBA pixels.
    ///
    /// Returns false without touching `pixels` when hidden or visually unchanged.
    /// Reuse the same vector to avoid allocating a new CPU buffer for every frame.
    /// The native host presents at [Self::pixel_size] and then reconciles native
    /// children using [crate::native_view::NativeViewHost::layout].
    pub fn draw(&mut self, pixels: &mut Vec<u8>) -> Result<bool, EmbeddedViewError> {
        if !self.visible {
            return Ok(false);
        }
        let update = self.shell.update();
        let owed = self.shell.take_frame_owed();
        if !(self.dirty || owed || update.visual_changed || self.shell.needs_redraw()) {
            return Ok(false);
        }
        let (width, height) = self.target.size();
        self.shell
            .renderer()
            .render(self.target.texture(), self.target.view(), width, height)
            .map_err(|error| EmbeddedViewError::Render(format!("{error:?}")))?;
        self.target.read_into(&self.device, &self.queue, pixels)?;
        self.dirty = false;
        Ok(true)
    }

    /// Physical dimensions of the image produced by [Self::draw].
    pub fn pixel_size(&self) -> (u32, u32) {
        self.target.size()
    }
}

fn validate_size(
    width: u32,
    height: u32,
    density: f32,
    limit: u32,
) -> Result<(), EmbeddedViewError> {
    if width == 0
        || height == 0
        || width > limit
        || height > limit
        || u64::from(width) * u64::from(height) > 16 * 1024 * 1024
        || !density.is_finite()
        || density <= 0.0
    {
        return Err(EmbeddedViewError::InvalidSize {
            width,
            height,
            density,
        });
    }
    Ok(())
}
