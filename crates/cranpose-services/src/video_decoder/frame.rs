//! A decoded picture that the app receives.

use std::{ffi::c_void, time::Duration};

#[cfg(target_os = "android")]
use super::image_reader::Image;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use super::pixel_buffer::Image;
#[cfg(not(any(target_os = "android", target_os = "macos", target_os = "ios")))]
use super::unavailable::Image;

/// What the decoder reports about one decoded picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    /// The timestamp of the access unit that the picture came from.
    pub timestamp: Duration,
    /// The metadata of that access unit.
    pub metadata: u64,
    /// How long the decoder took: from the access unit's submission to the picture's output.
    pub decode_time: Duration,
}

/// The part of a picture's buffer that holds the picture, in pixels. A decoder codes a picture
/// of 360 rows in a buffer of 368 rows, so a renderer shows only this part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropRect {
    /// The column of the part's left edge.
    pub x: u32,
    /// The row of the part's top edge.
    pub y: u32,
    /// The width of the part.
    pub width: u32,
    /// The height of the part.
    pub height: u32,
}

/// A decoded picture as a GPU image. On Android it is a hardware buffer that a renderer samples
/// without a copy. On macOS and iOS it is a `CVPixelBuffer` in an IOSurface that Metal samples
/// without a copy.
///
/// The picture's image stays with the decoder until the frame drops. A frame that lives long
/// keeps the decoder from using the image again, so an app drops each frame once it has drawn it.
pub struct VideoFrame {
    info: FrameInfo,
    size: (u32, u32),
    crop: CropRect,
    image: Image,
}

impl VideoFrame {
    #[cfg(any(target_os = "android", target_os = "macos", target_os = "ios"))]
    pub(super) fn new(info: FrameInfo, size: (u32, u32), crop: CropRect, image: Image) -> Self {
        Self {
            info,
            size,
            crop,
            image,
        }
    }

    /// The timestamp, the metadata and the decode time of the picture.
    pub fn info(&self) -> FrameInfo {
        self.info
    }

    /// The width and the height of the picture's buffer in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The part of the buffer that holds the picture.
    pub fn crop(&self) -> CropRect {
        self.crop
    }

    /// The picture's native buffer as a pointer: an `AHardwareBuffer*`, which a renderer imports
    /// as a GPU image. The pointer stays valid while this frame lives. Apps do not call this:
    /// the headset's backend does.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    pub fn as_hardware_buffer_ptr(&self) -> *mut c_void {
        self.image.hardware_buffer()
    }

    /// The picture's native buffer as a pointer: a `CVPixelBufferRef` in NV12, backed by an
    /// IOSurface that Metal samples. Its format is
    /// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange`, or the full-range format if the
    /// stream uses that range. The pointer stays valid while this frame lives. Apps do not call
    /// this: `PlayerSurface` of the `cranpose` crate imports it.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub fn as_pixel_buffer_ptr(&self) -> *mut c_void {
        self.image.pixel_buffer()
    }
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("info", &self.info)
            .field("size", &self.size)
            .field("crop", &self.crop)
            .finish_non_exhaustive()
    }
}
