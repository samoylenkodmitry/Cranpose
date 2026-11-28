// What a frame draws in place of what compiles, and which of its pixels
// may show it.

use cranpose_ui_graphics::RenderEffect;

use super::{FrameExecutor, PendingBackdrop, StandInMark, StandInPixels};
use crate::frame_graph::FrameCommandRecorder;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "stand-in steps in their file"
)]
impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    // The pixels drawn since `mark`.
    pub(super) fn pixels_since(&self, mark: StandInMark) -> StandInPixels {
        mark.pixels_until(self.renderer.stand_in_mark())
    }

    // The pixels of the frame's pages so far, and of the window pixels
    // they keep: a backdrop holds them.
    pub(super) fn page_pixels(&self) -> StandInPixels {
        if self.renderer.pipelines.stand_ins.kept() {
            StandInPixels::Interim
        } else {
            self.pixels_since(self.stand_ins_at_start)
        }
    }

    /// Whether `effect` over a `source_size` source draws a layer at
    /// `layer_pixel_rect` without waiting for a pipeline to compile. When
    /// it does not, what it misses is queued and the frame draws a stand-in.
    pub(super) fn effect_draws_now(
        &mut self,
        effect: &RenderEffect,
        layer_pixel_rect: [f32; 4],
        source_size: (u32, u32),
    ) -> bool {
        let renderer = &mut *self.renderer;
        let ready = renderer.effect_renderer.effect_ready(
            &renderer.device,
            effect,
            crate::effect_renderer::layer_pixels(layer_pixel_rect),
            source_size,
        );
        if !ready {
            renderer.pipelines.note_placeholder(&renderer.frame_stats);
        }
        ready
    }

    pub(super) fn backdrop_draws_now(&mut self, item: &PendingBackdrop<'_>) -> bool {
        self.effect_draws_now(
            item.effect,
            item.layer_pixel_rect(),
            item.capture_rect.pixel_size(),
        )
    }
}
