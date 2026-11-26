// One white texel: the content of every shaded mesh whose node draws nothing, so the mesh's shader
// colors its whole surface with no pass for the empty content.

use std::{cell::OnceCell, rc::Rc};

use crate::offscreen::OffscreenTarget;

// The texel, made when a scene first draws such a mesh.
#[derive(Default)]
pub(crate) struct WhiteTexel(OnceCell<Rc<OffscreenTarget>>);

impl WhiteTexel {
    pub(crate) fn texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Rc<OffscreenTarget> {
        Rc::clone(self.0.get_or_init(|| {
            let target = OffscreenTarget::new_labeled(
                device,
                wgpu::TextureFormat::Rgba8Unorm,
                1,
                1,
                "White Mesh Content",
            );
            // One write of 4 bytes for the process: no frame's upload counts need it
            crate::frame_graph::upload_texture(
                queue,
                target.texture().as_image_copy(),
                &[u8::MAX; 4],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            Rc::new(target)
        }))
    }
}
