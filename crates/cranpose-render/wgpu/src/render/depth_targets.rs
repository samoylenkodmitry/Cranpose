use super::DEPTH_FORMAT;
use crate::{idle_pool::IdlePool, scene_pass::SCENE_DEPTH_FORMAT};

const KEPT_INTERIOR_TARGETS: usize = 4;

const KEPT_SCENE_TARGETS: usize = 2;

// The transient depth buffers passes draw against, by target size.
#[derive(Default)]
pub(crate) struct DepthTargets {
    // For passes that lay opaque interiors down first, dropped once no
    // frame has used them for `crate::idle_pool::IDLE_FRAMES`.
    interior: IdlePool<((u32, u32), wgpu::TextureView)>,
    // The depth and stencil targets scenes' passes drew against.
    scene: IdlePool<((u32, u32), wgpu::TextureView)>,
}

impl DepthTargets {
    // The depth buffer for an interior pass into a target of `size`,
    // created on first use; the few sizes recent frames used are kept.
    pub(crate) fn interior(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> wgpu::TextureView {
        let view = match self.interior.take(|(kept, _)| *kept == size) {
            Some((_, view)) => view,
            None => transient_depth_target(device, ("Opaque interior depth", DEPTH_FORMAT), size),
        };
        self.interior
            .put((size, view.clone()), KEPT_INTERIOR_TARGETS, u64::MAX, |_| 0);
        view
    }

    // The depth and stencil a scene's pass draws against at `size`, kept
    // for the next frames of the same size.
    pub(crate) fn scene(&mut self, device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
        let view = match self.scene.take(|(kept, _)| *kept == size) {
            Some((_, view)) => view,
            None => transient_depth_target(
                device,
                ("Scene depth and stencil", SCENE_DEPTH_FORMAT),
                size,
            ),
        };
        self.scene
            .put((size, view.clone()), KEPT_SCENE_TARGETS, u64::MAX, |_| 0);
        view
    }

    pub(super) fn end_frame(&mut self) {
        self.interior.end_frame();
    }
}

// A depth target of `format` and `size` a pass draws against and drops,
// which a tiled GPU keeps in its tile memory.
fn transient_depth_target(
    device: &wgpu::Device,
    (label, format): (&'static str, wgpu::TextureFormat),
    size: (u32, u32),
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
