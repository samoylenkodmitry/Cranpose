//! The controllers' rays: a quad layer for each hand that shows one small static texture.
//!
//! The texture is written once. Each frame only the layers' poses change, so a ray costs the
//! app no draw.

use cranpose::GpuContext;
use openxr as xr;

use super::{
    session::{LayerSource, QuadLayer, XrSession},
    static_image::{premultiplied, static_image},
};
use crate::{beam_pose::quad_pose, ray::Beam};

// The texture's pixels across the beam and along it.
const ACROSS: u32 = 8;
const ALONG: u32 = 128;
// The beam's width in meters.
const WIDTH: f32 = 0.004;
// The beam's color in linear light: a bluish white.
const COLOR: [f32; 3] = [0.8, 0.9, 1.0];
// The beam's opacity at the controller and at its far end.
const NEAR_ALPHA: f32 = 0.1;
const FAR_ALPHA: f32 = 0.8;

pub(crate) struct RayLayers {
    swapchain: xr::Swapchain<xr::OpenGlEs>,
}

impl RayLayers {
    // Makes the swapchain and writes the beam's picture into its one image.
    pub(crate) fn new(
        xr_session: &XrSession,
        gpu: &GpuContext,
        formats: (u32, wgpu::TextureFormat),
    ) -> Result<Self, String> {
        let swapchain = static_image(
            xr_session,
            gpu,
            formats,
            (ACROSS, ALONG),
            &beam_pixels(xr_session.flips_rows()),
        )?;
        Ok(Self { swapchain })
    }

    // The layers of the beams, left hand then right, for a viewer's head at `head`. A hand with
    // no beam has no layer.
    pub(crate) fn layers<'a>(
        &'a self,
        beams: &[Option<Beam>; 2],
        head: [f32; 3],
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        let beams = *beams;
        beams
            .into_iter()
            .flatten()
            .filter(|beam| beam.length > 0.0)
            .map(move |beam| QuadLayer {
                swapchain: &self.swapchain,
                source: LayerSource::Drawn((ACROSS, ALONG)),
                pose: quad_pose(&beam, head),
                size: [WIDTH, beam.length],
            })
    }
}

// The beam's picture in rows of RGBA bytes with premultiplied alpha. The format is sRGB, so the
// color bytes are encoded. Alpha falls from the beam's middle to nothing at its edges and rises
// from the controller to the far end.
fn beam_pixels(flips_rows: bool) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((ACROSS * ALONG * 4) as usize);
    for row in 0..ALONG {
        // The compositor shows the first row at the far end when it flips rows, and at the
        // controller's end when it does not
        let from_controller = if flips_rows { ALONG - 1 - row } else { row };
        let along =
            NEAR_ALPHA + (FAR_ALPHA - NEAR_ALPHA) * from_controller as f32 / (ALONG - 1) as f32;
        for column in 0..ACROSS {
            let middle = (ACROSS - 1) as f32 / 2.0;
            let edge = (column as f32 - middle).abs() / middle;
            let across = 1.0 - edge * edge * (3.0 - 2.0 * edge);
            let alpha = along * across;
            pixels.extend(premultiplied(COLOR, alpha));
        }
    }
    pixels
}
