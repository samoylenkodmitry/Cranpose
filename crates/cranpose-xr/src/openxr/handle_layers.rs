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
    eye_camera::FirstRow,
    handle::{HANDLE_RADIUS, HandleFocus, HandleKind},
    projected_pass::{Picture, ProjectedLayer},
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
// The handle that a pointer would grab shows this much larger, and the one that a trigger presses
// larger still, so the hand sees which handle a pull takes and that the pull took it.
const HOVERED_SCALE: f32 = 1.3;
const PRESSED_SCALE: f32 = 1.6;

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
        let sphere = |kind| {
            let first_row = if xr_session.top_row_first() {
                FirstRow::Top
            } else {
                FirstRow::Bottom
            };
            let (side, pixels) = picture(kind, first_row);
            static_image(xr_session, gpu, format, (side, side), &pixels)
        };
        Ok(Self {
            move_sphere: sphere(HandleKind::Move)?,
            resize_sphere: sphere(HandleKind::Resize)?,
        })
    }

    // The layers of the handles at `slot`'s panel, in the order of `placed`.
    pub(crate) fn layers<'a>(
        &'a self,
        slot: &PanelSlot,
        active: [Option<HandleFocus>; 2],
        head: Option<[f32; 3]>,
    ) -> impl Iterator<Item = QuadLayer<'a>> + use<'a> {
        placed(slot, active, head).map(move |handle| QuadLayer {
            swapchain: match handle.kind {
                HandleKind::Move => &self.move_sphere,
                HandleKind::Resize => &self.resize_sphere,
            },
            source: LayerSource::Drawn((SIZE, SIZE)),
            pose: handle.pose,
            size: [handle.side; 2],
            space: None,
        })
    }
}

// A handle where it shows in a frame.
#[derive(Clone, Copy)]
pub(crate) struct Handle {
    kind: HandleKind,
    // The pose of the sphere's picture, which faces the head
    pose: Pose,
    // The picture's width and height in meters
    side: f32,
}

impl Handle {
    // The handle as the projection layer draws it in place of its layer.
    pub(crate) fn projected(self) -> ProjectedLayer {
        ProjectedLayer {
            picture: Picture::Handle(self.kind),
            pose: self.pose,
            size: [self.side; 2],
            radius: None,
        }
    }
}

// The handles at `slot`'s panel: the one that moves it if the panel is movable, then the one that
// resizes it if the panel is resizable. The handles face the head at `head`. A headset that tracks
// no head shows them facing the way the panel faces. A handle that a hand in `active` holds or
// would grab shows larger, and one that a trigger presses larger still.
pub(crate) fn placed(
    slot: &PanelSlot,
    active: [Option<HandleFocus>; 2],
    head: Option<[f32; 3]>,
) -> impl Iterator<Item = Handle> + use<> {
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
            let scale = active
                .iter()
                .flatten()
                .filter(|focus| focus.panel == slot.id && focus.kind == kind)
                .map(|focus| {
                    if focus.pressed {
                        PRESSED_SCALE
                    } else {
                        HOVERED_SCALE
                    }
                })
                .fold(1.0, f32::max);
            Handle {
                kind,
                pose,
                side: 2.0 * HANDLE_RADIUS * scale,
            }
        })
}

// The picture of the sphere of a handle of `kind`, `SIZE` pixels square, in rows of RGBA bytes
// from `first_row`.
pub(crate) fn picture(kind: HandleKind, first_row: FirstRow) -> (u32, Vec<u8>) {
    let color = match kind {
        HandleKind::Move => MOVE_COLOR,
        HandleKind::Resize => RESIZE_COLOR,
    };
    (SIZE, sphere_pixels(color, first_row == FirstRow::Top))
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
