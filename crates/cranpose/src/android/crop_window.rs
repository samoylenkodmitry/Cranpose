//! Where a decoded picture lies in its hardware buffer, in the texture coordinates that the
//! conversion pass samples with.

use crate::CropRect;

// The bytes of the shader's `Window`: eight floats.
pub(crate) const WINDOW_SIZE: u32 = 32;
const WINDOW_BYTES: usize = WINDOW_SIZE as usize;

// The coordinate of the picture's first pixel, the size of one pixel, and the lowest and the
// highest coordinate that the pass samples.
pub(crate) struct CropWindow {
    place: [f32; 4],
    inside: [f32; 4],
}

impl CropWindow {
    // The window of the part `crop` in a buffer of `buffer` pixels.
    pub(crate) fn new(crop: CropRect, buffer: (u32, u32)) -> Self {
        let (width, height) = (buffer.0 as f32, buffer.1 as f32);
        let (x, y) = (crop.x as f32, crop.y as f32);
        // The first and the last pixel's center are the extremes that a filter reads inside the
        // part. A filter of the chroma samples also reads the neighbors of the last pixel, so the
        // highest coordinate lies a pixel inside the part, and a part of one pixel keeps the
        // lowest one.
        let low = [x + 0.5, y + 0.5];
        let high = [
            (x + crop.width as f32 - 1.0).max(low[0]),
            (y + crop.height as f32 - 1.0).max(low[1]),
        ];
        Self {
            place: [x / width, y / height, 1.0 / width, 1.0 / height],
            inside: [
                low[0] / width,
                low[1] / height,
                high[0] / width,
                high[1] / height,
            ],
        }
    }

    pub(crate) fn to_bytes(&self) -> [u8; WINDOW_BYTES] {
        let mut bytes = [0; WINDOW_BYTES];
        let (words, _) = bytes.as_chunks_mut::<4>();
        for (word, value) in words
            .iter_mut()
            .zip(self.place.into_iter().chain(self.inside))
        {
            *word = value.to_ne_bytes();
        }
        bytes
    }
}

#[cfg(test)]
#[path = "tests/crop_window_tests.rs"]
mod tests;
