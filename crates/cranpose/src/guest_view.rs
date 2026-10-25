use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_render_wgpu::{WgpuRenderer, WgpuTextSystem};

use crate::{AppSettings, GpuContext};

/// Failure to size or draw a [`GuestView`].
#[derive(Debug, thiserror::Error)]
pub enum GuestViewError {
    /// Dimensions must be nonzero and fit the device's texture limit, and the
    /// density must be finite and positive.
    #[error("invalid guest view size {width} x {height} at density {density}")]
    InvalidSize {
        /// Requested pixel width.
        width: u32,
        /// Requested pixel height.
        height: u32,
        /// Requested pixels per logical point.
        density: f32,
    },
    /// Cranpose could not render the scene.
    #[error("render: {0}")]
    Render(String),
}

/// A Cranpose composition drawn into textures on another program's GPU
/// device, such as a game engine's HUD texture.
///
/// It starts no window or event loop. The host routes input through
/// [`Self::shell`] in logical coordinates, calls [`Self::draw`] once per host
/// frame and shows the texture it drew into. The frame is submitted to the
/// context's queue, so work the host submits there afterwards sees it.
pub struct GuestView {
    shell: AppShell<WgpuRenderer>,
    max_dimension: u32,
    size: (u32, u32),
    density: f32,
    dirty: bool,
    visible: bool,
}

impl GuestView {
    /// Creates a composition that draws `width` by `height` pixel textures of
    /// `format` at `density` pixels per logical point.
    pub fn new(
        settings: &AppSettings,
        gpu: &GpuContext,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        density: f32,
        content: impl FnMut() + 'static,
    ) -> Result<Self, GuestViewError> {
        let max_dimension = gpu.device.limits().max_texture_dimension_2d;
        validate_size(width, height, density, max_dimension)?;
        let mut renderer = WgpuRenderer::with_text_system(WgpuTextSystem::from_font_set(
            settings.resolve_font_set(),
        ));
        renderer.init_gpu(cranpose_render_wgpu::GpuTarget::on_adapter(
            &gpu.adapter,
            std::sync::Arc::clone(&gpu.device),
            std::sync::Arc::clone(&gpu.queue),
            format,
        ));
        let shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            crate::gpu_context::provide_gpu_context(gpu.clone(), content),
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );
        Ok(Self {
            shell,
            max_dimension,
            size: (width, height),
            density,
            dirty: true,
            visible: true,
        })
    }

    /// Accesses input, scheduling, semantics, text input and the app context.
    pub fn shell(&mut self) -> &mut AppShell<WgpuRenderer> {
        &mut self.shell
    }

    /// Sets the size of the textures [`Self::draw`] draws into. Returns
    /// whether the pixel size changed. Invalid values leave the previous size.
    pub fn resize(
        &mut self,
        width: u32,
        height: u32,
        density: f32,
    ) -> Result<bool, GuestViewError> {
        validate_size(width, height, density, self.max_dimension)?;
        let size_changed = self.size != (width, height);
        if !size_changed && self.density == density {
            return Ok(false);
        }
        self.shell
            .primary()
            .set_viewport(width as f32 / density, height as f32 / density);
        self.shell.set_density(density);
        self.shell.set_buffer_size(width, height);
        self.size = (width, height);
        self.density = density;
        self.dirty = true;
        Ok(size_changed)
    }

    /// Suspends drawing while hidden and cancels any active gesture.
    /// Becoming visible draws the next frame whatever changed.
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

    /// Advances the composition and, when it looks different, draws it into
    /// `texture` through `view`. The texture must have
    /// [`pixel_size`](Self::pixel_size) and the format the view was created
    /// with. Returns whether it drew.
    pub fn draw(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) -> Result<bool, GuestViewError> {
        if !self.visible {
            return Ok(false);
        }
        let update = self.shell.update();
        let owed = self.shell.take_frame_owed();
        if !(self.dirty || owed || update.visual_changed || self.shell.needs_redraw()) {
            return Ok(false);
        }
        let (width, height) = self.size;
        self.shell
            .renderer()
            .render(texture, view, width, height)
            .map_err(|error| GuestViewError::Render(format!("{error:?}")))?;
        self.dirty = false;
        Ok(true)
    }

    /// Physical dimensions of the textures [`Self::draw`] draws into.
    pub fn pixel_size(&self) -> (u32, u32) {
        self.size
    }
}

fn validate_size(width: u32, height: u32, density: f32, limit: u32) -> Result<(), GuestViewError> {
    if width == 0
        || height == 0
        || width > limit
        || height > limit
        || !density.is_finite()
        || density <= 0.0
    {
        return Err(GuestViewError::InvalidSize {
            width,
            height,
            density,
        });
    }
    Ok(())
}
