//! Image bitmap primitives used by render backends.

use std::{
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

use thiserror::Error;

use crate::{BlendMode, Color, Size};

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
    width: u32,
    height: u32,
    id: u64,
    opaque: bool,
    pixels: Arc<[u8]>,
}

/// Texture sampling mode for image primitives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageSampling {
    /// Preserve source texels exactly. Use this for atlases, pixel art, and UI skins.
    #[default]
    Nearest,
    /// Interpolate adjacent texels. Use this for photographic or continuously scaled images.
    Linear,
}

/// Simple image color filter model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorFilter {
    /// Compose-style tint using `BlendMode::SrcIn`.
    Tint(Color),
    /// Explicit per-channel modulation (multiply behavior).
    Modulate(Color),
    /// 4x5 color matrix in row-major order.
    ///
    /// Rows map output RGBA channels, columns map input RGBA plus constant term:
    /// `out = M * [r, g, b, a, 1]`.
    Matrix([f32; 20]),
}

impl ColorFilter {
    /// Creates a Compose-style tint filter (`SrcIn`).
    pub fn tint(color: Color) -> Self {
        Self::Tint(color)
    }

    /// Creates an explicit modulation filter that multiplies channels by `color`.
    pub fn modulate(color: Color) -> Self {
        Self::Modulate(color)
    }

    /// Creates a filter from a 4x5 color matrix.
    pub fn matrix(matrix: [f32; 20]) -> Self {
        Self::Matrix(matrix)
    }

    pub fn compose(self, next: ColorFilter) -> ColorFilter {
        ColorFilter::Matrix(compose_color_matrices(self.as_matrix(), next.as_matrix()))
    }

    pub fn as_matrix(self) -> [f32; 20] {
        match self {
            Self::Tint(tint) => [
                0.0,
                0.0,
                0.0,
                tint.r(),
                0.0,
                0.0,
                0.0,
                0.0,
                tint.g(),
                0.0,
                0.0,
                0.0,
                0.0,
                tint.b(),
                0.0,
                0.0,
                0.0,
                0.0,
                tint.a(),
                0.0,
            ],
            Self::Modulate(modulate) => [
                modulate.r(),
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                modulate.g(),
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                modulate.b(),
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                modulate.a(),
                0.0,
            ],
            Self::Matrix(matrix) => matrix,
        }
    }

    pub fn apply_rgba(self, rgba: [f32; 4]) -> [f32; 4] {
        apply_color_matrix(self.as_matrix(), rgba)
    }

    pub fn supports_gpu_vertex_modulation(self) -> bool {
        matches!(self, Self::Modulate(_))
    }

    pub fn gpu_vertex_tint(self) -> Option<[f32; 4]> {
        match self {
            Self::Modulate(tint) => Some([tint.r(), tint.g(), tint.b(), tint.a()]),
            _ => None,
        }
    }

    pub fn blend_mode(self) -> BlendMode {
        match self {
            Self::Tint(_) => BlendMode::SrcIn,
            Self::Modulate(_) => BlendMode::Modulate,
            Self::Matrix(_) => BlendMode::SrcOver,
        }
    }
}

fn apply_color_matrix(matrix: [f32; 20], rgba: [f32; 4]) -> [f32; 4] {
    let r = rgba[0];
    let g = rgba[1];
    let b = rgba[2];
    let a = rgba[3];
    [
        (matrix[0] * r + matrix[1] * g + matrix[2] * b + matrix[3] * a + matrix[4]).clamp(0.0, 1.0),
        (matrix[5] * r + matrix[6] * g + matrix[7] * b + matrix[8] * a + matrix[9]).clamp(0.0, 1.0),
        (matrix[10] * r + matrix[11] * g + matrix[12] * b + matrix[13] * a + matrix[14])
            .clamp(0.0, 1.0),
        (matrix[15] * r + matrix[16] * g + matrix[17] * b + matrix[18] * a + matrix[19])
            .clamp(0.0, 1.0),
    ]
}

fn compose_color_matrices(first: [f32; 20], second: [f32; 20]) -> [f32; 20] {
    let mut composed = [0.0f32; 20];
    for row in 0..4 {
        let row_base = row * 5;
        let s0 = second[row_base];
        let s1 = second[row_base + 1];
        let s2 = second[row_base + 2];
        let s3 = second[row_base + 3];
        let s4 = second[row_base + 4];

        composed[row_base] = s0 * first[0] + s1 * first[5] + s2 * first[10] + s3 * first[15];
        composed[row_base + 1] = s0 * first[1] + s1 * first[6] + s2 * first[11] + s3 * first[16];
        composed[row_base + 2] = s0 * first[2] + s1 * first[7] + s2 * first[12] + s3 * first[17];
        composed[row_base + 3] = s0 * first[3] + s1 * first[8] + s2 * first[13] + s3 * first[18];
        composed[row_base + 4] =
            s0 * first[4] + s1 * first[9] + s2 * first[14] + s3 * first[19] + s4;
    }
    composed
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

        let id = bitmap_content_id(width, height, pixels);
        let opaque = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == u8::MAX);
        Ok(Self {
            width,
            height,
            id,
            opaque,
            pixels: Arc::from(pixels),
        })
    }

    /// Content-derived bitmap identity used by renderer caches.
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

    /// Returns true when every source pixel has full alpha.
    pub fn is_opaque(&self) -> bool {
        self.opaque
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
        self.id() == other.id()
    }
}

impl Eq for ImageBitmap {}

impl Hash for ImageBitmap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

fn bitmap_content_id(width: u32, height: u32, pixels: &[u8]) -> u64 {
    let mut hasher = foldhash::quality::FixedState::default().build_hasher();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    pixels.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "tests/image_tests.rs"]
mod tests;
