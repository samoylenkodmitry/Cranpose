//! A frame drawn against a depth that the host owns.

use crate::{WgpuRenderer, WgpuRendererError};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the host's depth in its own file"
)]
impl WgpuRenderer {
    /// Renders like [`render`](Self::render), with the 3D scenes of the frame drawn against `depth`
    /// and leaving their depth there, for a host that hands the depth on, such as an XR compositor.
    ///
    /// `depth` is a view of a texture of [`SCENE_DEPTH_FORMAT`] with `width` by `height` pixels and
    /// the render attachment usage. The scenes clear it and write the distance of what they
    /// draw as `1 - near / z`, where `z` is the depth along the camera's axis and `near` is
    /// [`Camera::near`](cranpose_ui_graphics::space::Camera::near): 0 at the near plane, 1 at
    /// infinity, and 1 where no scene drew. Only a scene with
    /// [`Multisample::Off`](cranpose_ui_graphics::space::Multisample::Off) draws against it, and
    /// each scene pass clears it again, so a frame that draws several scenes leaves the depth of
    /// the last one. A scene drawn into an intermediate layer, such as one under a blur, uses depth
    /// of the renderer's own.
    pub fn render_with_scene_depth(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        (width, height): (u32, u32),
    ) -> Result<(), WgpuRendererError> {
        self.backend
            .sync_mut()?
            .renderer
            .depth_targets
            .host_scene(Some(((width, height), depth.clone())));
        let drawn = self.render(texture, view, width, height);
        if let Ok(sync) = self.backend.sync_mut() {
            sync.renderer.depth_targets.host_scene(None);
        }
        drawn
    }
}
