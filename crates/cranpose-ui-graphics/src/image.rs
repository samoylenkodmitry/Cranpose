//! Image bitmap primitives used by render backends.

use std::{
    any::Any,
    borrow::Cow,
    hash::{BuildHasher, Hash, Hasher},
    panic::AssertUnwindSafe,
    sync::{Arc, atomic::Ordering},
};

use portable_atomic::AtomicU64;
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

/// Storage format of an immutable bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImagePixelFormat {
    /// Four bytes per pixel, ordered red, green, blue, alpha.
    Rgba8,
    /// One alpha byte per pixel and a constant, unpremultiplied RGB color.
    Alpha8 {
        /// RGB channels shared by every pixel, including transparent pixels.
        color: [u8; 3],
    },
    /// No pixels in memory: a GPU texture another renderer draws, made by
    /// [`ExternalTexture`]. Software renderers draw it transparent.
    External,
}

/// Immutable image data used by UI primitives and render backends.
#[derive(Clone, Debug)]
pub struct ImageBitmap {
    data: Arc<ImageBitmapData>,
}

#[derive(Debug)]
struct ImageBitmapData {
    width: u32,
    height: u32,
    id: u64,
    opaque: bool,
    format: ImagePixelFormat,
    storage: ImageStorage,
}

enum ImageStorage {
    Pixels(Box<[u8]>),
    External { key: u64, handle: ExternalHandle },
}

type ExternalHandle = Arc<AssertUnwindSafe<Box<dyn Any + Send + Sync>>>;

impl std::fmt::Debug for ImageStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pixels(pixels) => write!(formatter, "Pixels({} bytes)", pixels.len()),
            Self::External { key, .. } => write!(formatter, "External({key:#x})"),
        }
    }
}

/// A GPU texture that another renderer, such as a game engine, draws into,
/// shown through Cranpose's image primitives.
///
/// The handle belongs to the renderer that draws the scene: the wgpu
/// renderer draws a `wgpu::TextureView` and samples it as stored, so an sRGB
/// texture needs a non-sRGB view. Software renderers draw it transparent.
///
/// Draw [`frame`](Self::frame) with the number of the texture's current
/// content. Image ids include that number, so every cache keyed by an image
/// id draws the new content once the number changes.
#[derive(Clone)]
pub struct ExternalTexture {
    key: u64,
    width: u32,
    height: u32,
    opaque: bool,
    handle: ExternalHandle,
}

impl std::fmt::Debug for ExternalTexture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalTexture")
            .field("key", &self.key)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl ExternalTexture {
    /// Wraps a renderer's texture handle of `width` by `height` pixels.
    /// `opaque` promises that every pixel the texture holds has full alpha.
    pub fn new(
        width: u32,
        height: u32,
        opaque: bool,
        handle: impl Any + Send + Sync,
    ) -> Result<Self, ImageBitmapError> {
        if width == 0 || height == 0 {
            return Err(ImageBitmapError::InvalidDimensions);
        }
        let serial = NEXT_EXTERNAL_TEXTURE.fetch_add(1, Ordering::Relaxed);
        let mut hasher = foldhash::quality::FixedState::default().build_hasher();
        ImagePixelFormat::External.hash(&mut hasher);
        serial.hash(&mut hasher);
        Ok(Self {
            key: hasher.finish(),
            width,
            height,
            opaque,
            handle: Arc::new(AssertUnwindSafe(Box::new(handle))),
        })
    }

    /// The bitmap that draws content number `frame` of this texture.
    pub fn frame(&self, frame: u64) -> ImageBitmap {
        ImageBitmap {
            data: Arc::new(ImageBitmapData {
                width: self.width,
                height: self.height,
                id: self.key ^ frame.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                opaque: self.opaque,
                format: ImagePixelFormat::External,
                storage: ImageStorage::External {
                    key: self.key,
                    handle: Arc::clone(&self.handle),
                },
            }),
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether a bitmap from [`frame`](Self::frame), or another clone of this
    /// texture, is still alive. A renderer draws the texture only through such
    /// a bitmap, so once none is alive, no draw of it is waiting for the GPU
    /// queue.
    pub fn bitmaps_alive(&self) -> bool {
        Arc::strong_count(&self.handle) > 1
    }
}

static NEXT_EXTERNAL_TEXTURE: AtomicU64 = AtomicU64::new(0);

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
    /// Creates a bitmap by taking ownership of tightly packed RGBA8 pixels.
    /// Any spare capacity in the pixel buffer is released.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, ImageBitmapError> {
        Self::from_pixels(width, height, ImagePixelFormat::Rgba8, pixels)
    }

    /// Copies tightly packed RGBA8 pixels into a new bitmap.
    pub fn from_rgba8_slice(
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Self, ImageBitmapError> {
        Self::from_pixels(width, height, ImagePixelFormat::Rgba8, pixels)
    }

    /// Takes ownership of one alpha byte per pixel with a constant RGB color.
    /// Any spare capacity in the alpha buffer is released.
    pub fn from_alpha8(
        width: u32,
        height: u32,
        color: [u8; 3],
        alpha: Vec<u8>,
    ) -> Result<Self, ImageBitmapError> {
        Self::from_pixels(width, height, ImagePixelFormat::Alpha8 { color }, alpha)
    }

    fn from_pixels(
        width: u32,
        height: u32,
        format: ImagePixelFormat,
        pixels: impl AsRef<[u8]> + Into<Box<[u8]>>,
    ) -> Result<Self, ImageBitmapError> {
        if width == 0 || height == 0 {
            return Err(ImageBitmapError::InvalidDimensions);
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or(ImageBitmapError::DimensionsTooLarge)?;
        let expected = match format {
            ImagePixelFormat::Rgba8 => expected,
            ImagePixelFormat::Alpha8 { .. } => expected / 4,
            ImagePixelFormat::External => 0,
        };

        let bytes = pixels.as_ref();
        if bytes.len() != expected {
            return Err(ImageBitmapError::PixelDataLengthMismatch {
                expected,
                actual: bytes.len(),
            });
        }

        let id = bitmap_content_id(width, height, format, bytes);
        let opaque = match format {
            ImagePixelFormat::Rgba8 => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == u8::MAX),
            ImagePixelFormat::Alpha8 { .. } => bytes.iter().all(|alpha| *alpha == u8::MAX),
            ImagePixelFormat::External => false,
        };
        Ok(Self {
            data: Arc::new(ImageBitmapData {
                width,
                height,
                id,
                opaque,
                format,
                storage: ImageStorage::Pixels(pixels.into()),
            }),
        })
    }

    /// Content-derived bitmap identity used by renderer caches. Each frame
    /// of an [`ExternalTexture`] has its own id.
    pub fn id(&self) -> u64 {
        self.data.id
    }

    /// Identity of the bitmap's storage: the same for every frame of an
    /// [`ExternalTexture`], equal to [`Self::id`] for pixel bitmaps.
    pub fn storage_id(&self) -> u64 {
        match &self.data.storage {
            ImageStorage::External { key, .. } => *key,
            ImageStorage::Pixels(_) => self.data.id,
        }
    }

    /// The renderer handle of an [`ExternalTexture`] frame, when it is a `T`.
    pub fn external_handle<T: 'static>(&self) -> Option<&T> {
        match &self.data.storage {
            ImageStorage::External { handle, .. } => handle.downcast_ref(),
            ImageStorage::Pixels(_) => None,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.data.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.data.height
    }

    /// Returns the raw pixel bytes in [`Self::format`], empty for an
    /// [`ExternalTexture`] frame.
    pub fn pixels(&self) -> &[u8] {
        match &self.data.storage {
            ImageStorage::Pixels(pixels) => pixels,
            ImageStorage::External { .. } => &[],
        }
    }

    /// Returns the storage format of [`Self::pixels`].
    pub fn format(&self) -> ImagePixelFormat {
        self.data.format
    }

    /// Returns the unpremultiplied RGBA8 pixel at a row-major index;
    /// transparent for an [`ExternalTexture`] frame.
    ///
    /// # Panics
    /// Panics when `index` is outside the image.
    pub fn rgba8_pixel(&self, index: usize) -> [u8; 4] {
        match self.format() {
            ImagePixelFormat::Rgba8 => self.pixels().as_chunks::<4>().0[index],
            ImagePixelFormat::Alpha8 { color: [r, g, b] } => [r, g, b, self.pixels()[index]],
            ImagePixelFormat::External => [0; 4],
        }
    }

    /// Returns tightly packed RGBA8 bytes, borrowing RGBA images and expanding
    /// alpha masks; empty for an [`ExternalTexture`] frame.
    pub fn rgba8_pixels(&self) -> Cow<'_, [u8]> {
        match self.format() {
            ImagePixelFormat::Rgba8 | ImagePixelFormat::External => Cow::Borrowed(self.pixels()),
            ImagePixelFormat::Alpha8 { color: [r, g, b] } => {
                let mut pixels = Vec::with_capacity(self.pixels().len() * 4);
                for &alpha in self.pixels() {
                    pixels.extend_from_slice(&[r, g, b, alpha]);
                }
                Cow::Owned(pixels)
            }
        }
    }

    /// Returns true when every source pixel has full alpha.
    pub fn is_opaque(&self) -> bool {
        self.data.opaque
    }

    /// Returns intrinsic size in logical units.
    pub fn intrinsic_size(&self) -> Size {
        Size {
            width: self.data.width as f32,
            height: self.data.height as f32,
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

fn bitmap_content_id(width: u32, height: u32, format: ImagePixelFormat, pixels: &[u8]) -> u64 {
    let mut hasher = foldhash::quality::FixedState::default().build_hasher();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    format.hash(&mut hasher);
    pixels.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
#[path = "tests/image_tests.rs"]
mod tests;
