//! The empty layer that the first shown frame repeats to try how many layers the runtime takes.
//!
//! The layer shows a small picture whose pixels are all clear, so the copies of the layer show
//! nothing whether the runtime takes the frame or not. The swapchain goes once the trial is over.

use cranpose::GpuContext;
use openxr as xr;

use super::{
    graphics::{Api, SwapchainFormat},
    session::{LayerSource, QuadLayer, XrSession},
    static_image::static_image,
};
use crate::Pose;

// The picture's side in pixels.
const SIDE: u32 = 4;

pub(crate) struct TrialLayer {
    swapchain: xr::Swapchain<Api>,
}

impl TrialLayer {
    // The layer, while the session still has to try how many layers the runtime takes.
    pub(crate) fn new(
        xr_session: &XrSession,
        gpu: &GpuContext,
        format: SwapchainFormat,
    ) -> Result<Option<Self>, String> {
        if !xr_session.layer_trial_due() {
            return Ok(None);
        }
        let clear = [0_u8; (SIDE * SIDE * 4) as usize];
        let swapchain = static_image(xr_session, gpu, format, (SIDE, SIDE), &clear)?;
        Ok(Some(Self { swapchain }))
    }

    // One copy of the layer: a clear millimeter at the room's origin.
    pub(crate) fn layer(&self) -> QuadLayer<'_> {
        QuadLayer {
            swapchain: &self.swapchain,
            source: LayerSource::Drawn((SIDE, SIDE)),
            pose: Pose::IDENTITY,
            size: [0.001, 0.001],
            space: None,
        }
    }
}
