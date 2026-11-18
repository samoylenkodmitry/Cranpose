//! The handles of movable and resizable panels: a quad layer for each with one small static
//! picture of a sphere.
//!
//! The pictures are written once: a bluish white sphere for the handle that moves a panel and an
//! amber one for the handle that resizes it. Each frame the layers turn their faces to the head,
//! so a handle looks round from every side and costs the app no draw.

use cranpose::GpuContext;
use openxr as xr;

use super::{
    graphics::{Api, SwapchainFormat},
    session::{LayerSource, QuadLayer, XrSession},
    static_image::{premultiplied, static_image},
};
use crate::{
    Pose,
    billboard_pose::billboard_pose,
    handle::{HANDLE_RADIUS, HandleKind},
    space::PanelSlot,
    vector::{dot, normalize},
};

// The picture's pixels across and down.
const SIZE: u32 = 128;
// The color in linear light of the sphere that moves a panel, a bluish white, and of the sphere
// that resizes it, an amber. Both have the same opacity.
const MOVE_COLOR: [f32; 3] = [0.8, 0.9, 1.0];
const RESIZE_COLOR: [f32; 3] = [1.0, 0.75, 0.35];
const ALPHA: f32 = 0.85;
// The share of the color that the sphere's dark side keeps.
const AMBIENT: f32 = 0.3;
// The way to the light in the quad's axes: right, up and toward the viewer. The light leans to
// the upper left of the viewer.
const LIGHT: [f32; 3] = [-0.35, 0.45, 1.0];
// The sharpness of the sphere's highlight and its strength.
const SHININESS: f32 = 40.0;
const HIGHLIGHT: f32 = 0.5;
// The handle that the pointer would grab or carries shows this much larger.
const ACTIVE_SCALE: f32 = 1.3;

pub(crate) struct HandleLayers {
    move_sphere: xr::Swapchain<Api>,
    resize_sphere: xr::Swapchain<Api>,
}

impl HandleLayers {
    pub(crate) fn new(
        xr_session: &XrSession,
        gpu: &GpuContext,
        format: SwapchainFormat,
    ) -> Result<Self, String> {
        let sphere = |color| {
            static_image(
                xr_session,
                gpu,
                format,
                (SIZE, SIZE),
                &sphere_pixels(color, xr_session.top_row_first()),
            )
        };
        Ok(Self {
            move_sphere: sphere(MOVE_COLOR)?,
            resize_sphere: sphere(RESIZE_COLOR)?,
        })
    }

    // The layers of the handles at `slot`'s panel: the one that moves it if the panel is movable,
    // then the one that resizes it if the panel is resizable. The layers face the head at `head`.
    // A headset that tracks no head shows them facing the way the panel faces. The handle of
    // `active` shows larger.
    pub(crate) fn layers<'a>(
        &'a self,
        slot: &PanelSlot,
        active: Option<(u64, HandleKind)>,
        head: Option<[f32; 3]>,
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        let slot = *slot;
        HandleKind::ALL
            .into_iter()
            .filter(move |kind| kind.offered_by(&slot))
            .map(move |kind| {
                let center = kind.center(&slot);
                let pose = head.map_or(
                    Pose {
                        position: center,
                        orientation: slot.pose.orientation,
                    },
                    |head| billboard_pose(center, head),
                );
                let scale = if active == Some((slot.id, kind)) {
                    ACTIVE_SCALE
                } else {
                    1.0
                };
                let side = 2.0 * HANDLE_RADIUS * scale;
                QuadLayer {
                    swapchain: match kind {
                        HandleKind::Move => &self.move_sphere,
                        HandleKind::Resize => &self.resize_sphere,
                    },
                    source: LayerSource::Drawn((SIZE, SIZE)),
                    pose,
                    size: [side, side],
                }
            })
    }
}

// The picture of a sphere of `color` in rows of RGBA bytes with premultiplied alpha. The sphere
// touches the picture's edges and has a soft edge one pixel wide. The light comes from the
// viewer's upper left, so the picture shows a lit ball from any side.
fn sphere_pixels(color: [f32; 3], top_row_first: bool) -> Vec<u8> {
    let half = SIZE as f32 / 2.0;
    let light = normalize(LIGHT);
    // The half-way direction between the light and the viewer, for the highlight
    let middle = normalize([light[0], light[1], light[2] + 1.0]);
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for row in 0..SIZE {
        // The compositor shows the first row at the quad's top when the first row is the top one,
        // and at its bottom when it is the bottom one
        let from_top = if top_row_first { row } else { SIZE - 1 - row };
        let up = half - (from_top as f32 + 0.5);
        for column in 0..SIZE {
            let right = column as f32 + 0.5 - half;
            let coverage = (half - right.hypot(up)).clamp(0.0, 1.0);
            let (x, y) = (right / half, up / half);
            let normal = [x, y, (1.0 - x * x - y * y).max(0.0).sqrt()];
            let diffuse = dot(normal, light).max(0.0);
            let highlight = HIGHLIGHT * dot(normal, middle).max(0.0).powf(SHININESS);
            let body = AMBIENT + (1.0 - AMBIENT) * diffuse;
            let color = color.map(|channel| (channel * body + highlight).min(1.0));
            pixels.extend(premultiplied(color, ALPHA * coverage));
        }
    }
    pixels
}
