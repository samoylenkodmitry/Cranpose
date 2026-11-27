//! The `CVPixelBuffer` of a decoded picture.
#![expect(
    unsafe_code,
    reason = "a CVPixelBuffer is a reference-counted C object that the picture owns one count of"
)]

use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use objc2_core_foundation::{CFRetained, CFString, CFType};
use objc2_core_video::{
    CVImageBufferGetCleanRect, CVPixelBuffer, CVPixelBufferGetHeight,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, kCVImageBufferColorPrimaries_EBU_3213,
    kCVImageBufferColorPrimaries_ITU_R_709_2, kCVImageBufferColorPrimaries_ITU_R_2020,
    kCVImageBufferColorPrimaries_SMPTE_C, kCVImageBufferColorPrimariesKey,
    kCVImageBufferTransferFunction_ITU_R_709_2, kCVImageBufferTransferFunction_ITU_R_2020,
    kCVImageBufferTransferFunction_ITU_R_2100_HLG, kCVImageBufferTransferFunction_Linear,
    kCVImageBufferTransferFunction_SMPTE_240M_1995,
    kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ, kCVImageBufferTransferFunction_UseGamma,
    kCVImageBufferTransferFunction_sRGB, kCVImageBufferTransferFunctionKey,
    kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrix_ITU_R_709_2,
    kCVImageBufferYCbCrMatrix_ITU_R_2020, kCVImageBufferYCbCrMatrixKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};

use super::{
    ColorPrimaries, ColorRange, ColorTransfer, CropRect, FrameInfo, VideoColorInfo, VideoFrame,
    VideoRotation, sps::StreamMetadata,
};

/// Wraps a valid `CVPixelBufferRef` (retaining one reference count) into a zero-copy
/// [`VideoFrame`], reading its clean aperture and CoreVideo color metadata without heap
/// allocation.
///
/// # Safety
/// `pixel_buffer` must be null or a valid `CVPixelBufferRef` backed by an IOSurface.
pub unsafe fn video_frame_from_pixel_buffer_ptr(
    pixel_buffer: *mut c_void,
    info: FrameInfo,
    rotation: VideoRotation,
) -> Option<VideoFrame> {
    // SAFETY: caller guarantees `pixel_buffer` is a valid CVPixelBufferRef.
    let image = unsafe { Image::retain_from_ptr(pixel_buffer)? };
    let size = image.size();
    let crop = image.crop();
    let color = image.color(StreamMetadata::default(), None);
    Some(VideoFrame::new(info, size, crop, color, rotation, image))
}

// The pictures of a decoder that the decoder or the app holds. The count keeps the app from
// holding every picture that the decoder's pool has.
pub(super) type LivePictures = Arc<AtomicUsize>;

// A decoded picture's buffer. It holds one count of the buffer, which the decoder's pool gets
// back when the picture drops.
pub(super) struct Image {
    buffer: CFRetained<CVPixelBuffer>,
    live: Option<LivePictures>,
}

// SAFETY: a CVPixelBuffer's reference count changes atomically, and the decoder never writes
// the pixels of a picture that it has output.
unsafe impl Send for Image {}
// SAFETY: as for Send, and the buffer's size does not change.
unsafe impl Sync for Image {}

impl Image {
    // Counts the picture among the live ones.
    pub(super) fn new(buffer: CFRetained<CVPixelBuffer>, live: &LivePictures) -> Self {
        live.fetch_add(1, Ordering::Relaxed);
        Self {
            buffer,
            live: Some(Arc::clone(live)),
        }
    }

    // Retains one reference count of a valid `CVPixelBufferRef` with zero heap allocation.
    pub(super) unsafe fn retain_from_ptr(ptr: *mut c_void) -> Option<Self> {
        let ptr = NonNull::new(ptr.cast::<CVPixelBuffer>())?;
        // SAFETY: caller guarantees `ptr` is a valid CVPixelBufferRef.
        let buffer = unsafe { CFRetained::retain(ptr) };
        Some(Self { buffer, live: None })
    }

    pub(super) fn pixel_buffer(&self) -> *mut c_void {
        NonNull::from(&*self.buffer).as_ptr().cast()
    }

    pub(super) fn size(&self) -> (u32, u32) {
        let width = CVPixelBufferGetWidth(&self.buffer);
        let height = CVPixelBufferGetHeight(&self.buffer);
        (
            u32::try_from(width).unwrap_or(u32::MAX),
            u32::try_from(height).unwrap_or(u32::MAX),
        )
    }

    // The clean aperture reported by CoreVideo, clamped to the buffer dimensions.
    pub(super) fn crop(&self) -> CropRect {
        let (buf_w, buf_h) = self.size();
        let clean = CVImageBufferGetCleanRect(&self.buffer);
        let x = round_to_u32(clean.origin.x);
        let y = round_to_u32(clean.origin.y);
        let width = round_to_u32(clean.size.width);
        let height = round_to_u32(clean.size.height);
        if width > 0
            && height > 0
            && x.checked_add(width).is_some_and(|right| right <= buf_w)
            && y.checked_add(height).is_some_and(|bottom| bottom <= buf_h)
        {
            CropRect {
                x,
                y,
                width,
                height,
            }
        } else {
            CropRect {
                x: 0,
                y: 0,
                width: buf_w,
                height: buf_h,
            }
        }
    }

    // Resolves the picture's color metadata from CoreVideo buffer format and attachments without
    // heap allocation on standard streams, falling back to SPS VUI, container config, and defaults.
    pub(super) fn color(
        &self,
        stream: StreamMetadata,
        config_color: Option<VideoColorInfo>,
    ) -> VideoColorInfo {
        let (_, height) = self.size();
        let format = CVPixelBufferGetPixelFormatType(&self.buffer);
        let range = if format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            || format == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange
        {
            ColorRange::Full
        } else if format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            || format == kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
        {
            ColorRange::Limited
        } else {
            stream
                .range
                .or_else(|| config_color.map(|c| c.range))
                .unwrap_or(ColorRange::Limited)
        };
        let primaries = attachment_primaries(&self.buffer)
            .or(stream.primaries)
            .or_else(|| config_color.map(|c| c.primaries))
            .unwrap_or_else(|| ColorPrimaries::default_for_height(height));
        let transfer = attachment_transfer(&self.buffer)
            .or(stream.transfer)
            .or_else(|| config_color.map(|c| c.transfer))
            .unwrap_or(ColorTransfer::Sdr);
        VideoColorInfo {
            primaries,
            range,
            transfer,
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        if let Some(live) = &self.live {
            live.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to finite 0..=u32::MAX; lint only fires under workspace clippy budgets"
)]
fn round_to_u32(value: f64) -> u32 {
    if !value.is_finite() {
        return 0;
    }
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

fn attachment_cf_string(buffer: &CVPixelBuffer, key: &CFString) -> Option<CFRetained<CFType>> {
    // SAFETY: `key` is a CoreVideo constant CFString, and the attachment mode is omitted.
    unsafe { buffer.attachment(key, ptr::null_mut()) }
}

fn attachment_primaries(buffer: &CVPixelBuffer) -> Option<ColorPrimaries> {
    // SAFETY: both keys are CoreVideo constants.
    let (matrix_key, primaries_key) = unsafe {
        (
            kCVImageBufferYCbCrMatrixKey,
            kCVImageBufferColorPrimariesKey,
        )
    };
    attachment_cf_string(buffer, matrix_key)
        .as_deref()
        .and_then(|value| value.downcast_ref::<CFString>())
        .and_then(match_cv_primaries)
        .or_else(|| {
            attachment_cf_string(buffer, primaries_key)
                .as_deref()
                .and_then(|value| value.downcast_ref::<CFString>())
                .and_then(match_cv_primaries)
        })
}

fn match_cv_primaries(value: &CFString) -> Option<ColorPrimaries> {
    // SAFETY: CoreVideo exposes immutable constant CFString references.
    let (m709, m601, m2020, p709, p2020, p_smpte_c, p_ebu) = unsafe {
        (
            kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            kCVImageBufferYCbCrMatrix_ITU_R_601_4,
            kCVImageBufferYCbCrMatrix_ITU_R_2020,
            kCVImageBufferColorPrimaries_ITU_R_709_2,
            kCVImageBufferColorPrimaries_ITU_R_2020,
            kCVImageBufferColorPrimaries_SMPTE_C,
            kCVImageBufferColorPrimaries_EBU_3213,
        )
    };
    if value == m709 || value == p709 {
        return Some(ColorPrimaries::Bt709);
    }
    if value == m601 || value == p_smpte_c || value == p_ebu {
        return Some(ColorPrimaries::Bt601);
    }
    if value == m2020 || value == p2020 {
        return Some(ColorPrimaries::Bt2020);
    }
    // Cold fallback for CoreVideo numeric tags (`YCbCrMatrix#<N>`, `ColorPrimaries#<N>`).
    let name = value.to_string();
    name.strip_prefix("YCbCrMatrix#")
        .and_then(|n| n.parse::<u8>().ok())
        .and_then(ColorPrimaries::from_matrix_h273)
        .or_else(|| {
            name.strip_prefix("ColorPrimaries#")
                .and_then(|n| n.parse::<u8>().ok())
                .and_then(ColorPrimaries::from_h273)
        })
}

fn attachment_transfer(buffer: &CVPixelBuffer) -> Option<ColorTransfer> {
    // SAFETY: CoreVideo exposes immutable constant CFString references.
    let (key, pq, hlg, t709, t2020, srgb, smpte240, gamma, linear) = unsafe {
        (
            kCVImageBufferTransferFunctionKey,
            kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ,
            kCVImageBufferTransferFunction_ITU_R_2100_HLG,
            kCVImageBufferTransferFunction_ITU_R_709_2,
            kCVImageBufferTransferFunction_ITU_R_2020,
            kCVImageBufferTransferFunction_sRGB,
            kCVImageBufferTransferFunction_SMPTE_240M_1995,
            kCVImageBufferTransferFunction_UseGamma,
            kCVImageBufferTransferFunction_Linear,
        )
    };
    let attachment = attachment_cf_string(buffer, key)?;
    let value = attachment.downcast_ref::<CFString>()?;
    if value == pq {
        return Some(ColorTransfer::Pq);
    }
    if value == hlg {
        return Some(ColorTransfer::Hlg);
    }
    if value == t709
        || value == t2020
        || value == srgb
        || value == smpte240
        || value == gamma
        || value == linear
    {
        return Some(ColorTransfer::Sdr);
    }
    value
        .to_string()
        .strip_prefix("TransferFunction#")
        .and_then(|n| n.parse::<u8>().ok())
        .and_then(ColorTransfer::from_h273)
}
