//! Affine texture-coordinate transform for video clean aperture cropping and clockwise rotation.
//!
//! Shared between the WGSL bi-planar YUV converter (Apple, Windows, Linux) and the Android Vulkan
//! YCbCr pass so every platform applies identical crop and 0/90/180/270-degree rotation math.

use cranpose_services::{CropRect, VideoRotation};

// Normalized 2D affine mapping from target fragment coordinates to source buffer UVs, plus the
// clamped chroma UV box that prevents bilinear sampling from reading buffer padding.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoSampleTransform {
    // xy: source UV at target (0, 0), zw: source UV delta across target +X
    pub(crate) origin_du: [f32; 4],
    // xy: source UV delta across target +Y, zw: (1 / target_w, 1 / target_h)
    pub(crate) dv_inv_target: [f32; 4],
    // xy: min safe chroma UV inside crop, zw: max safe chroma UV inside crop
    pub(crate) inside: [f32; 4],
}

impl VideoSampleTransform {
    pub(crate) const fn target_size(crop: CropRect, rotation: VideoRotation) -> (u32, u32) {
        let width = if crop.width == 0 { 1 } else { crop.width };
        let height = if crop.height == 0 { 1 } else { crop.height };
        if rotation.swaps_axes() {
            (height, width)
        } else {
            (width, height)
        }
    }

    pub(crate) fn new(crop: CropRect, buffer: (u32, u32), rotation: VideoRotation) -> Self {
        let bw = buffer.0.max(1) as f32;
        let bh = buffer.1.max(1) as f32;
        let cx = crop.x as f32;
        let cy = crop.y as f32;
        let cw = crop.width.max(1) as f32;
        let ch = crop.height.max(1) as f32;

        let target_size = Self::target_size(crop, rotation);
        let inv_tw = 1.0 / target_size.0 as f32;
        let inv_th = 1.0 / target_size.1 as f32;

        let (ox, oy, dux, duy, dvx, dvy) = match rotation {
            VideoRotation::Degrees0 => (cx / bw, cy / bh, cw / bw, 0.0, 0.0, ch / bh),
            VideoRotation::Degrees90 => (cx / bw, (cy + ch) / bh, 0.0, -ch / bh, cw / bw, 0.0),
            VideoRotation::Degrees180 => {
                ((cx + cw) / bw, (cy + ch) / bh, -cw / bw, 0.0, 0.0, -ch / bh)
            }
            VideoRotation::Degrees270 => ((cx + cw) / bw, cy / bh, 0.0, ch / bh, -cw / bw, 0.0),
        };

        // Keep chroma filtering strictly inside the clean aperture so subsampled edge taps never
        // pull in decoder alignment padding.
        let low_x = cx + 0.5;
        let low_y = cy + 0.5;
        let high_x = (cx + cw - 1.0).max(low_x);
        let high_y = (cy + ch - 1.0).max(low_y);

        Self {
            origin_du: [ox, oy, dux, duy],
            dv_inv_target: [dvx, dvy, inv_tw, inv_th],
            inside: [low_x / bw, low_y / bh, high_x / bw, high_y / bh],
        }
    }
}
