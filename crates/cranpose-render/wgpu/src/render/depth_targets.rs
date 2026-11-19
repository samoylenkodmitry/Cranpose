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
    // The depth and stencil a host gave for the frame that draws now, whose scenes draw against it
    // and leave their depth in it.
    hosted_scene: Option<((u32, u32), wgpu::TextureView)>,
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
            None => transient_attachment(device, ("Opaque interior depth", DEPTH_FORMAT, 1), size),
        };
        self.interior
            .put((size, view.clone()), KEPT_INTERIOR_TARGETS, u64::MAX, |_| 0);
        view
    }

    // Has the scenes of the next frame draw against `depth`, a view of `size` that the host owns,
    // so that the host reads the depth they leave. `None` goes back to targets of the renderer's
    // own, which the passes drop.
    pub(crate) fn host_scene(&mut self, hosted: Option<((u32, u32), wgpu::TextureView)>) {
        self.hosted_scene = hosted;
    }

    // The depth and stencil a scene's pass draws against at `size`, and whether the pass keeps its
    // depth: the host's view when it has the size, else a target of the renderer's own, kept for
    // the next frames of the same size.
    pub(crate) fn scene(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
    ) -> (wgpu::TextureView, bool) {
        if let Some((hosted, view)) = &self.hosted_scene
            && *hosted == size
        {
            return (view.clone(), true);
        }
        let view = match self.scene.take(|(kept, _)| *kept == size) {
            Some((_, view)) => view,
            None => transient_attachment(
                device,
                ("Scene depth and stencil", SCENE_DEPTH_FORMAT, 1),
                size,
            ),
        };
        self.scene
            .put((size, view.clone()), KEPT_SCENE_TARGETS, u64::MAX, |_| 0);
        (view, false)
    }

    pub(super) fn end_frame(&mut self) {
        self.interior.end_frame();
    }
}

// A target of `format`, `samples` deep and of `size`, a pass draws into and
// drops, which a tiled GPU keeps in its tile memory.
pub(super) fn transient_attachment(
    device: &wgpu::Device,
    (label, format, samples): (&'static str, wgpu::TextureFormat, u32),
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
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}
