//! Curved panels: layers of the headset's compositor that show a swapchain image on a cylinder
//! (`XR_KHR_composition_layer_cylinder`).

use std::{f32::consts::TAU, ptr};

use openxr as xr;

use super::{graphics::Api, session::posef};
use crate::Pose;

// A layer that shows a whole swapchain image on a curved panel.
pub(crate) struct CylinderLayer<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<Api>,
    pub(crate) image_size: (u32, u32),
    // The pose of the panel's center, which faces the cylinder's axis.
    pub(crate) pose: Pose,
    // The cylinder's radius in meters.
    pub(crate) radius: f32,
    // The panel's arc length and height in meters.
    pub(crate) size: [f32; 2],
    // The tracked space that `pose` is in, or `None` for the frame's space: the room
    pub(crate) space: Option<xr::sys::Space>,
}

impl CylinderLayer<'_> {
    // The compositor's struct for the layer in its own space, or in `space` for a layer in the
    // room. It names the swapchain by its handle, so the struct is valid for as long as the
    // swapchain lives.
    pub(crate) fn to_raw(&self, space: xr::sys::Space) -> xr::sys::CompositionLayerCylinderKHR {
        // OpenXR places the layer by its cylinder's axis, and the visible part lies on the
        // axis's -z side
        let axis = self.pose.then(&Pose::at([0.0, 0.0, self.radius]));
        let (width, height) = self.image_size;
        let image = xr::SwapchainSubImage::new()
            .swapchain(self.swapchain)
            .image_rect(xr::Rect2Di {
                offset: xr::Offset2Di { x: 0, y: 0 },
                extent: xr::Extent2Di {
                    width: width as i32,
                    height: height as i32,
                },
            });
        let mut raw = xr::CompositionLayerCylinderKHR::<Api>::new()
            .layer_flags(xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA)
            .eye_visibility(xr::EyeVisibility::BOTH)
            .sub_image(image)
            .pose(posef(&axis))
            .radius(self.radius)
            // A full turn is not a section of the cylinder: the angle stays below it
            .central_angle((self.size[0] / self.radius).min(TAU - 0.01))
            .aspect_ratio(self.size[0] / self.size[1])
            .into_raw();
        raw.space = self.space.unwrap_or(space);
        raw.next = ptr::null();
        raw
    }
}
