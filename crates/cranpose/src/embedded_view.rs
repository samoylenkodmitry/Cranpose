use std::sync::Arc;

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use crate::{
    AppSettings, GpuContext, GuestView, GuestViewError, frame_readback::FrameTarget,
    offscreen_instance::OFFSCREEN_INSTANCE,
};

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
/// Offscreen components share GPU backend initialization across worker threads.
///
/// [Self::draw] renders to a reusable GPU target and reads pixels only when
/// changed. This portable adapter copies pixels; it is not a zero-copy native
/// surface. Hosts with a Rust GPU integration draw into their own texture
/// through [`GuestView`] instead.
pub struct EmbeddedView {
    view: GuestView,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    target: FrameTarget,
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
        validate_readback_size(width, height, density)?;
        let adapter = pollster::block_on(OFFSCREEN_INSTANCE.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            },
        ))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Cranpose embedded view"),
                required_features: cranpose_render_wgpu::optional_device_features(&adapter),
                required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
                memory_hints: crate::gpu_limits::mobile_memory_hints(),
                ..Default::default()
            }))?;
        let gpu = GpuContext {
            instance: OFFSCREEN_INSTANCE.clone(),
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
        };
        let view = GuestView::new(
            settings,
            &gpu,
            FRAME_FORMAT,
            width,
            height,
            density,
            content,
        )?;
        let target = FrameTarget::new(&gpu.device, width, height, FRAME_FORMAT);
        Ok(Self {
            view,
            device: gpu.device,
            queue: gpu.queue,
            target,
        })
    }

    /// Accesses input, scheduling, semantics, text input and the app context.
    ///
    /// Install a frame waker, route pointer events in logical coordinates, and
    /// honor `frame_schedule()` after each draw. Native controls keep their own
    /// touch, focus and accessibility handling.
    pub fn shell(&mut self) -> &mut AppShell<WgpuRenderer> {
        self.view.shell()
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
        validate_readback_size(width, height, density)?;
        if self.view.resize(width, height, density)? {
            self.target = FrameTarget::new(&self.device, width, height, FRAME_FORMAT);
        }
        Ok(())
    }

    /// Suspends drawing while detached or hidden and cancels any active gesture.
    ///
    /// Becoming visible requests a fresh frame without destroying the composition.
    pub fn set_visible(&mut self, visible: bool) {
        self.view.set_visible(visible);
    }

    /// Advances the component and writes tightly packed premultiplied RGBA pixels.
    ///
    /// Returns false without touching `pixels` when hidden or visually unchanged.
    /// Reuse the same vector to avoid allocating a new CPU buffer for every frame.
    /// The native host presents at [Self::pixel_size] and then reconciles native
    /// children using [crate::native_view::NativeViewHost::layout].
    pub fn draw(&mut self, pixels: &mut Vec<u8>) -> Result<bool, EmbeddedViewError> {
        if !self.view.draw(self.target.texture(), self.target.view())? {
            return Ok(false);
        }
        self.target.read_into(&self.device, &self.queue, pixels)?;
        Ok(true)
    }

    /// Physical dimensions of the image produced by [Self::draw].
    pub fn pixel_size(&self) -> (u32, u32) {
        self.view.pixel_size()
    }
}

impl From<GuestViewError> for EmbeddedViewError {
    fn from(error: GuestViewError) -> Self {
        match error {
            GuestViewError::InvalidSize {
                width,
                height,
                density,
            } => Self::InvalidSize {
                width,
                height,
                density,
            },
            GuestViewError::Render(message) => Self::Render(message),
        }
    }
}

fn validate_readback_size(width: u32, height: u32, density: f32) -> Result<(), EmbeddedViewError> {
    if u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return Err(EmbeddedViewError::InvalidSize {
            width,
            height,
            density,
        });
    }
    Ok(())
}
