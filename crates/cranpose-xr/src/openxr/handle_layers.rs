//! The handles under movable panels: a quad layer for each with one small static texture.

use cranpose::GpuContext;
use openxr as xr;

use super::{
    session::{LayerSource, QuadLayer, XrSession},
    static_image::{premultiplied, static_image},
};
use crate::{
    carry::handle_pose,
    space::{Mobility, PanelSlot},
};

// The texture's pixels across the handle and down it.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 8;
const ALPHA: f32 = 0.7;
// The handle that the pointer would grab or carries shows this much larger.
const ACTIVE_SCALE: f32 = 1.3;

pub(crate) struct HandleLayers {
    swapchain: xr::Swapchain<xr::OpenGlEs>,
}

impl HandleLayers {
    pub(crate) fn new(
        xr_session: &XrSession,
        gpu: &GpuContext,
        formats: (u32, wgpu::TextureFormat),
    ) -> Result<Self, String> {
        let swapchain = static_image(xr_session, gpu, formats, (WIDTH, HEIGHT), &pill_pixels())?;
        Ok(Self { swapchain })
    }

    // The layers of the movable panels' handles. The handle of `active` shows larger.
    pub(crate) fn layers<'a>(
        &'a self,
        panels: &'a [PanelSlot],
        active: Option<u64>,
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        panels
            .iter()
            .filter(|slot| slot.mobility == Mobility::Movable)
            .map(move |slot| {
                let (pose, size) = handle_pose(slot);
                let scale = if active == Some(slot.id) {
                    ACTIVE_SCALE
                } else {
                    1.0
                };
                QuadLayer {
                    swapchain: &self.swapchain,
                    source: LayerSource::Drawn((WIDTH, HEIGHT)),
                    pose,
                    size: size.map(|side| side * scale),
                }
            })
    }
}

// A white pill with round ends and a soft edge one pixel wide.
fn pill_pixels() -> Vec<u8> {
    let radius = HEIGHT as f32 / 2.0;
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for row in 0..HEIGHT {
        for column in 0..WIDTH {
            let x = column as f32 + 0.5;
            let y = row as f32 + 0.5;
            let center_x = x.clamp(radius, WIDTH as f32 - radius);
            let distance = ((x - center_x).powi(2) + (y - radius).powi(2)).sqrt();
            let coverage = (radius - distance + 0.5).clamp(0.0, 1.0);
            pixels.extend(premultiplied([1.0; 3], ALPHA * coverage));
        }
    }
    pixels
}
