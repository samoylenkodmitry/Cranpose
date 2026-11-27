//! Push constants describing crop and rotation for the Android Vulkan YCbCr pass.

use cranpose_services::{CropRect, VideoRotation};

use crate::video_transform::VideoSampleTransform;

// The bytes of the shader's `Window`: twelve floats (three vec4s).
pub(crate) const WINDOW_SIZE: u32 = 48;
const WINDOW_BYTES: usize = WINDOW_SIZE as usize;

// Affine source UV transform and chroma clamp bounds for one decoded picture.
pub(crate) struct CropWindow {
    transform: VideoSampleTransform,
}

impl CropWindow {
    pub(crate) fn new(crop: CropRect, buffer: (u32, u32), rotation: VideoRotation) -> Self {
        Self {
            transform: VideoSampleTransform::new(crop, buffer, rotation),
        }
    }

    pub(crate) fn to_bytes(&self) -> [u8; WINDOW_BYTES] {
        let mut bytes = [0; WINDOW_BYTES];
        let (words, _) = bytes.as_chunks_mut::<4>();
        let values = self
            .transform
            .origin_du
            .into_iter()
            .chain(self.transform.dv_inv_target)
            .chain(self.transform.inside);
        for (word, value) in words.iter_mut().zip(values) {
            *word = value.to_ne_bytes();
        }
        bytes
    }
}

#[cfg(test)]
#[path = "tests/crop_window_tests.rs"]
mod tests;
