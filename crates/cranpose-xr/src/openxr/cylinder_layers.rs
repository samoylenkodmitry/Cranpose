//! Curved panels: layers of the headset's compositor that show a swapchain image on a cylinder
//! (`XR_KHR_composition_layer_cylinder`).
#![expect(
    unsafe_code,
    reason = "layers built from raw structs with an extension struct in their next chain"
)]

use std::{f32::consts::TAU, ptr};

use openxr as xr;

use super::session::posef;
use crate::Pose;

// A layer that shows a whole swapchain image on a curved panel.
pub(crate) struct CylinderLayer<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<xr::OpenGlEs>,
    pub(crate) image_size: (u32, u32),
    // The pose of the panel's center, which faces the cylinder's axis.
    pub(crate) pose: Pose,
    // The cylinder's radius in meters.
    pub(crate) radius: f32,
    // The panel's arc length and height in meters.
    pub(crate) size: [f32; 2],
}

// The compositor's layers for `layers` in `space`. `flip` is the layout that says the images
// start with their top row. It stays alive until the frame ends.
pub(crate) fn raw_layers<'a>(
    layers: &[CylinderLayer<'a>],
    space: &'a xr::Space,
    flip: Option<&'a xr::sys::CompositionLayerImageLayoutFB>,
) -> Vec<xr::CompositionLayerCylinderKHR<'a, xr::OpenGlEs>> {
    layers
        .iter()
        .map(|layer| {
            // OpenXR places the layer by its cylinder's axis, and the visible part lies on the
            // axis's -z side
            let axis = layer.pose.then(&Pose::at([0.0, 0.0, layer.radius]));
            let (width, height) = layer.image_size;
            let image = xr::SwapchainSubImage::new()
                .swapchain(layer.swapchain)
                .image_rect(xr::Rect2Di {
                    offset: xr::Offset2Di { x: 0, y: 0 },
                    extent: xr::Extent2Di {
                        width: width as i32,
                        height: height as i32,
                    },
                });
            let mut raw = xr::CompositionLayerCylinderKHR::<xr::OpenGlEs>::new()
                .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
                .space(space)
                .eye_visibility(xr::EyeVisibility::BOTH)
                .sub_image(image)
                .pose(posef(&axis))
                .radius(layer.radius)
                // A full turn is not a section of the cylinder: the angle stays below it
                .central_angle((layer.size[0] / layer.radius).min(TAU - 0.01))
                .aspect_ratio(layer.size[0] / layer.size[1])
                .into_raw();
            raw.next = flip.map_or(ptr::null(), |flip| ptr::from_ref(flip).cast());
            // SAFETY: the struct names a live space and swapchain, and its next chain holds a
            // layout struct that outlives the frame's end.
            unsafe { xr::CompositionLayerCylinderKHR::from_raw(raw) }
        })
        .collect()
}
