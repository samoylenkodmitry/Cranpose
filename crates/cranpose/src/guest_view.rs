use cranpose_app_shell::{AppShell, FrameSchedule, RootId, default_root_key};
use cranpose_core::NodeId;
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{GpuTarget, WgpuRenderer, WgpuTextSystem};

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
    gpu: GpuContext,
    format: wgpu::TextureFormat,
    text_system: WgpuTextSystem,
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
        let text_system = WgpuTextSystem::from_text_fonts(settings.resolve_fonts());
        let mut renderer = WgpuRenderer::with_text_system(text_system.clone());
        renderer.init_gpu(GpuTarget::on_adapter(
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
        shell.use_system_clipboard();
        Ok(Self {
            shell,
            gpu: gpu.clone(),
            format,
            text_system,
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

    /// What the view asks its host for: the shell's schedule while visible,
    /// and nothing while hidden, as a hidden view neither updates nor draws.
    pub fn frame_schedule(&self) -> FrameSchedule {
        if self.visible {
            self.shell.frame_schedule()
        } else {
            FrameSchedule::default()
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
        // The update runs for the whole composition. What it changed in a window root owes that
        // window a frame, and the primary root owes one only for what it changed itself.
        self.shell.update();
        let owed = self.shell.take_frame_owed();
        if !(self.dirty || owed || self.shell.needs_redraw()) {
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

    /// Gives the window root `root` a view of its own, drawn by [`Self::draw_window`] into
    /// textures of `width` by `height` pixels at `density` pixels per logical point, with a
    /// transparent background. `root` is the node of a
    /// [`Modifier::window_root`](crate::Modifier::window_root) in this composition, as
    /// [`Self::shell`]'s `window_roots` lists it. The root lays out into the size that its
    /// [`WindowRootDescriptor`](crate::WindowRootDescriptor) reports, which the caller sets to the
    /// view's logical size (`width / density` by `height / density`) before the next frame.
    ///
    /// A window root with a view of its own is not part of the textures [`Self::draw`] draws, and
    /// the composition stays one: state, effects and locals are shared with the primary root.
    pub fn open_window(
        &mut self,
        root: NodeId,
        (width, height): (u32, u32),
        density: f32,
    ) -> Result<(), GuestViewError> {
        validate_size(width, height, density, self.max_dimension)?;
        let mut renderer = WgpuRenderer::with_text_system(self.text_system.clone());
        renderer.set_root_scale(density);
        renderer.set_transparent_background(true);
        renderer.init_gpu(GpuTarget::on_adapter(
            &self.gpu.adapter,
            std::sync::Arc::clone(&self.gpu.device),
            std::sync::Arc::clone(&self.gpu.queue),
            self.format,
        ));
        self.shell.add_window_surface(
            root,
            renderer,
            (width, height),
            (width as f32 / density, height as f32 / density),
        );
        Ok(())
    }

    /// Takes the view of the window root `root` away.
    pub fn close_window(&mut self, root: NodeId) {
        self.shell.remove_window_surface(root);
    }

    /// Updates the composition and draws the window root `root` into `texture` through `view`,
    /// whatever changed. The texture has the size that [`Self::open_window`] was given and the
    /// format of the view. The update runs no frame callbacks: the frame's one tick is that of
    /// [`Self::draw`], so animations and effects that wait for a frame advance once for each
    /// frame of the host, whatever the number of draws of window roots.
    ///
    /// With `depth`, the root's 3D scenes draw against it and leave their depth in it: a view of
    /// a texture of [`SCENE_DEPTH_FORMAT`](cranpose_render_wgpu::SCENE_DEPTH_FORMAT) of the same
    /// size, with the render attachment usage. A scene's camera and the states its graphics
    /// layers read apply to the next draw, so one composition drawn twice with a state set
    /// between the draws shows two views of the same content, such as the two eyes of a headset.
    pub fn draw_window(
        &mut self,
        root: NodeId,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        depth: Option<&wgpu::TextureView>,
    ) -> Result<(), GuestViewError> {
        self.shell.update_without_frame();
        let mut surface = self.shell.surface(RootId::Window(root)).ok_or_else(|| {
            GuestViewError::Render(format!("no view for the window root {root:?}"))
        })?;
        let size = (texture.width(), texture.height());
        let renderer = surface.renderer();
        match depth {
            Some(depth) => renderer.render_with_scene_depth(texture, view, depth, size),
            None => renderer.render(texture, view, size.0, size.1),
        }
        .map_err(|error| GuestViewError::Render(format!("{error:?}")))
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
