use cranpose_ui_graphics::space::Multisample;

use super::depth_targets::transient_attachment;
use crate::{idle_pool::IdlePool, scene_pass::SCENE_DEPTH_FORMAT};

const KEPT_SAMPLED_TARGETS: usize = 2;

// The colour, depth and stencil, several samples deep, that the passes of
// multisampled scenes draw into, by size; dropped once no frame has used
// them for `crate::idle_pool::IDLE_FRAMES`, as they take four times the
// memory of a single-sample target where tile memory does not hold them.
#[derive(Default)]
pub(crate) struct SampledTargets {
    kept: IdlePool<SampledTarget>,
}

struct SampledTarget {
    size: (u32, u32),
    samples: Multisample,
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
}

impl SampledTargets {
    // The colour of `format` and the depth and stencil a scene's pass of
    // `samples` draws into at `size`, created on first use.
    pub(crate) fn scene(
        &mut self,
        device: &wgpu::Device,
        (size, format, samples): ((u32, u32), wgpu::TextureFormat, Multisample),
    ) -> (wgpu::TextureView, wgpu::TextureView) {
        let target = match self
            .kept
            .take(|kept| kept.size == size && kept.samples == samples)
        {
            Some(target) => target,
            None => SampledTarget {
                size,
                samples,
                colour: transient_attachment(
                    device,
                    ("Scene samples", format, samples.samples()),
                    size,
                ),
                depth: transient_attachment(
                    device,
                    (
                        "Scene sampled depth and stencil",
                        SCENE_DEPTH_FORMAT,
                        samples.samples(),
                    ),
                    size,
                ),
            },
        };
        let views = (target.colour.clone(), target.depth.clone());
        self.kept.put(target, KEPT_SAMPLED_TARGETS, u64::MAX, |_| 0);
        views
    }

    pub(super) fn end_frame(&mut self) {
        self.kept.end_frame();
    }
}
