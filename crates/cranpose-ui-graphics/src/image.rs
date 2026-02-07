//! Image bitmap primitives used by render backends.

use crate::{Color, Size};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

static NEXT_IMAGE_BITMAP_ID: AtomicU64 = AtomicU64::new(1);

/// Errors returned while constructing an [`ImageBitmap`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImageBitmapError {
    #[error("image dimensions must be greater than zero")]
    InvalidDimensions,
    #[error("image dimensions are too large")]
    DimensionsTooLarge,
    #[error("pixel data length mismatch: expected {expected} bytes, got {actual}")]
    PixelDataLengthMismatch { expected: usize, actual: usize },
}

/// Immutable RGBA image data used by UI primitives and render backends.
#[derive(Clone, Debug)]
pub struct ImageBitmap {
    id: u64,
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
}

/// Simple image color filter model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorFilter {
    /// Multiplies sampled RGBA channels by the tint color.
    Tint(Color),
}

impl ColorFilter {
    /// Creates a tint filter that multiplies sampled channels by `color`.
    pub fn tint(color: Color) -> Self {
        Self::Tint(color)
    }

    pub fn apply_rgba(self, rgba: [f32; 4]) -> [f32; 4] {
        match self {
            Self::Tint(tint) => [
                rgba[0] * tint.r(),
                rgba[1] * tint.g(),
                rgba[2] * tint.b(),
                rgba[3] * tint.a(),
            ],
        }
    }
}

impl ImageBitmap {
    /// Creates a bitmap from tightly packed RGBA8 pixels.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageBitmapError> {
        Self::from_rgba8_slice(width, height, &pixels)
    }

    /// Creates a bitmap from tightly packed RGBA8 pixels.
    pub fn from_rgba8_slice(
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Self, ImageBitmapError> {
        if width == 0 || height == 0 {
            return Err(ImageBitmapError::InvalidDimensions);
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or(ImageBitmapError::DimensionsTooLarge)?;

        if pixels.len() != expected {
            return Err(ImageBitmapError::PixelDataLengthMismatch {
                expected,
                actual: pixels.len(),
            });
        }

        Ok(Self {
            id: NEXT_IMAGE_BITMAP_ID.fetch_add(1, Ordering::Relaxed),
            width,
            height,
            pixels: Arc::from(pixels),
        })
    }

    /// Stable bitmap identity used by caches.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns the raw RGBA8 pixel data.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns intrinsic size in logical units.
    pub fn intrinsic_size(&self) -> Size {
        Size {
            width: self.width as f32,
            height: self.height as f32,
        }
    }
}

impl PartialEq for ImageBitmap {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ImageBitmap {}

impl Hash for ImageBitmap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rgba8_accepts_valid_data() {
        let bitmap = ImageBitmap::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])
            .expect("valid bitmap");

        assert_eq!(bitmap.width(), 2);
        assert_eq!(bitmap.height(), 1);
        assert_eq!(bitmap.pixels().len(), 8);
    }

    #[test]
    fn from_rgba8_rejects_zero_dimensions() {
        let err = ImageBitmap::from_rgba8(0, 2, vec![]).expect_err("must fail");
        assert_eq!(err, ImageBitmapError::InvalidDimensions);
    }

    #[test]
    fn from_rgba8_rejects_wrong_pixel_length() {
        let err = ImageBitmap::from_rgba8(2, 2, vec![0; 15]).expect_err("must fail");
        assert_eq!(
            err,
            ImageBitmapError::PixelDataLengthMismatch {
                expected: 16,
                actual: 15,
            }
        );
    }

    #[test]
    fn from_rgba8_slice_accepts_valid_data() {
        let pixels = [255u8, 0, 0, 255];
        let bitmap = ImageBitmap::from_rgba8_slice(1, 1, &pixels).expect("valid bitmap");
        assert_eq!(bitmap.pixels(), &pixels);
    }

    #[test]
    fn ids_are_unique() {
        let a = ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255]).expect("bitmap a");
        let b = ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255]).expect("bitmap b");
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn intrinsic_size_matches_dimensions() {
        let bitmap = ImageBitmap::from_rgba8(3, 4, vec![255; 3 * 4 * 4]).expect("bitmap");
        assert_eq!(bitmap.intrinsic_size(), Size::new(3.0, 4.0));
    }

    #[test]
    fn tint_filter_multiplies_channels() {
        let filter = ColorFilter::tint(Color::from_rgba_u8(128, 255, 64, 128));
        let tinted = filter.apply_rgba([1.0, 0.5, 1.0, 1.0]);
        assert!((tinted[0] - (128.0 / 255.0)).abs() < 1e-5);
        assert!((tinted[1] - 0.5).abs() < 1e-5);
        assert!((tinted[2] - (64.0 / 255.0)).abs() < 1e-5);
        assert!((tinted[3] - (128.0 / 255.0)).abs() < 1e-5);
    }

    #[test]
    fn tint_constructor_matches_variant() {
        let color = Color::from_rgba_u8(10, 20, 30, 40);
        assert_eq!(ColorFilter::tint(color), ColorFilter::Tint(color));
    }
}
