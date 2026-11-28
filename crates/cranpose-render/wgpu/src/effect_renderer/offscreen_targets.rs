use super::EffectRenderer;
use crate::{gpu_stats::FrameStats, offscreen::OffscreenTarget};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the offscreen targets of the effect renderer in their file"
)]
impl EffectRenderer {
    pub(crate) fn max_texture_dim(&self) -> u32 {
        self.offscreen_pool.max_texture_dim()
    }

    pub(crate) fn acquire_offscreen(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        stats: Option<&FrameStats>,
    ) -> OffscreenTarget {
        self.offscreen_pool.acquire(device, width, height, stats)
    }

    pub(crate) fn release_offscreen(&mut self, target: OffscreenTarget) {
        self.offscreen_pool.release(target);
    }

    // Ends a frame. The pool lets go of the targets no frame took for a
    // while. The shader cache lets go of the modules its builds are done
    // with.
    pub(crate) fn end_frame(&mut self) {
        self.offscreen_pool.end_frame();
        self.shader_cache.end_frame();
    }

    pub(crate) fn retained_offscreen_count(&self) -> usize {
        self.offscreen_pool.pool_size()
    }

    pub(crate) fn retained_offscreen_bytes(&self) -> usize {
        self.offscreen_pool.estimated_bytes()
    }
}
