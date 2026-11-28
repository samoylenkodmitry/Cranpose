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
    /// `depth` is a view of a texture of [`SCENE_DEPTH_FORMAT`](crate::SCENE_DEPTH_FORMAT) with
    /// `width` by `height` pixels and the render attachment usage. The scenes clear it and write
    /// the distance of what they draw as `1 - near / z`, where `z` is the depth along the camera's
    /// axis and `near` is
    /// [`Camera::near`](cranpose_ui_graphics::space::Camera::near): 0 at the near plane, 1 at
    /// infinity, and 1 where no scene drew. Each scene pass clears it again, so a frame that draws
    /// several scenes leaves the depth of the last one. A scene drawn into an intermediate layer,
    /// such as one under a blur, uses depth of the renderer's own. The frame draws whole, so `depth`
    /// takes the scenes' depth whatever the frame changed. It does so whatever `depth` held before,
    /// such as an older frame in the image of a depth swapchain.
    ///
    /// A scene with [`Multisample::Off`](cranpose_ui_graphics::space::Multisample::Off) stores its
    /// depth in its own pass. A scene with four samples cannot, because a render pass holds one
    /// sample count and wgpu has no depth resolve. Such a scene that covers a page holding only
    /// its transparent clear averages its colour straight into the page, then a second pass with
    /// one sample draws its opaque surfaces and volumes again with the depth pipelines. Each pixel
    /// holds the depth of its centre, so a pixel that an edge covers in part carries the depth of
    /// the surface at its centre, or the far plane where none covers the centre. Any other scene
    /// of four samples leaves the far plane.
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
