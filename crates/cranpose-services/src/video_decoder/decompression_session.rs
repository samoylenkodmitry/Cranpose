//! The VideoToolbox session that decodes the pictures of one stream format.
#![expect(
    unsafe_code,
    reason = "VideoToolbox and CoreMedia are C APIs whose objects the bindings hand out as references"
)]

use std::{
    ptr::{self, NonNull},
    slice,
};

use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags,
    CMVideoFormatDescriptionCreateFromH264ParameterSets,
    CMVideoFormatDescriptionCreateFromHEVCParameterSets, kCMBlockBufferAssureMemoryNowFlag,
    kCMTimeInvalid,
};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
#[cfg(target_os = "macos")]
use objc2_video_toolbox::kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder;
use objc2_video_toolbox::{
    VTDecodeFrameFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
    kVTDecompressionPropertyKey_RealTime,
};

use super::{DecodeOrder, DecoderError, VideoCodec, annex_b::LENGTH_SIZE};

// The pictures that the decoder outputs are NV12 (8-bit) or P010 (10-bit) in an IOSurface that
// Metal samples, in the range of the stream. Bi-planar YCbCr is what the hardware decoder writes:
// a request for BGRA makes VideoToolbox convert each picture, which took 2.0 to 2.7 ms for a
// picture of 2560 by 1440 pixels against 1.0 to 1.2 ms without the conversion. The renderer
// converts a picture to RGB in one pass.
const OUTPUT_FORMATS: [u32; 4] = [
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
];

// The timescale of the presentation times that the decoder takes and returns: microseconds.
pub(super) const TIMESCALE: i32 = 1_000_000;

// The format of a stream: its codec's parameter sets, as CoreMedia keeps them.
pub(super) struct Format {
    description: CFRetained<CMFormatDescription>,
    sets: Vec<Vec<u8>>,
}

// SAFETY: a format description is immutable after its creation, and CoreFoundation changes its
// reference count atomically.
unsafe impl Send for Format {}

impl Format {
    // Makes the format that `sets` describe. The sets come in the order of the stream.
    pub(super) fn new<'a>(
        codec: VideoCodec,
        sets: impl Iterator<Item = &'a [u8]>,
    ) -> Result<Self, DecoderError> {
        let sets: Vec<Vec<u8>> = sets.map(<[u8]>::to_vec).collect();
        let pointers: Vec<NonNull<u8>> = sets
            .iter()
            .map(|set| NonNull::from(set.as_slice()).cast())
            .collect();
        let sizes: Vec<usize> = sets.iter().map(Vec::len).collect();
        let (Some(pointers_in), Some(sizes_in)) = (
            NonNull::new(pointers.as_ptr().cast_mut()),
            NonNull::new(sizes.as_ptr().cast_mut()),
        ) else {
            return Err(DecoderError::NeedsKeyframe);
        };
        let header = i32::try_from(LENGTH_SIZE).unwrap_or(4);
        let mut description = ptr::null();
        // SAFETY: the pointers name `sets`, whose bytes outlive the call, in a count that the
        // sizes match. CoreMedia copies them. The output is a valid pointer.
        let status = unsafe {
            match codec {
                VideoCodec::H264 => CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    None,
                    sets.len(),
                    pointers_in,
                    sizes_in,
                    header,
                    NonNull::from(&mut description),
                ),
                VideoCodec::Hevc => CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                    None,
                    sets.len(),
                    pointers_in,
                    sizes_in,
                    header,
                    None,
                    NonNull::from(&mut description),
                ),
                VideoCodec::Av1 | VideoCodec::Vp9 => return Err(DecoderError::NoDecoder(codec)),
            }
        };
        let description = NonNull::new(description.cast_mut())
            .filter(|_| status == 0)
            // SAFETY: the function returns a description that the caller owns, in the output.
            .map(|description| unsafe { CFRetained::from_raw(description) })
            .ok_or_else(|| {
                DecoderError::Configure(format!("the parameter sets give no format: {status}"))
            })?;
        Ok(Self { description, sets })
    }

    // Whether `sets` are the parameter sets that made this format.
    pub(super) fn is_made_of<'a>(&self, sets: impl Iterator<Item = &'a [u8]>) -> bool {
        sets.eq(self.sets.iter().map(Vec::as_slice))
    }

    // A sample of one picture, whose bytes `fill` writes: `len` bytes of length-prefixed
    // units. The sample's presentation time is `pts_us` microseconds.
    pub(super) fn sample(
        &self,
        len: usize,
        fill: impl FnOnce(&mut [u8]),
        pts_us: u64,
    ) -> Result<CFRetained<CMSampleBuffer>, DecoderError> {
        let failed = |what: &str, status| DecoderError::Failed(format!("{what}: {status}"));
        let mut block = ptr::null_mut();
        // SAFETY: with no memory block given, CoreMedia allocates `len` bytes at once. The
        // output is a valid pointer.
        let status = unsafe {
            CMBlockBuffer::create_with_memory_block(
                None,
                ptr::null_mut(),
                len,
                None,
                ptr::null(),
                0,
                len,
                kCMBlockBufferAssureMemoryNowFlag,
                NonNull::from(&mut block),
            )
        };
        let block = NonNull::new(block)
            .filter(|_| status == 0)
            // SAFETY: the function returns a block that the caller owns, in the output.
            .map(|block| unsafe { CFRetained::from_raw(block) })
            .ok_or_else(|| failed("the block buffer", status))?;
        let mut bytes = ptr::null_mut();
        // SAFETY: the output is a valid pointer, and the others may be null.
        let status = unsafe {
            block.data_pointer(
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                NonNull::from(&mut bytes).as_ptr(),
            )
        };
        let bytes = NonNull::new(bytes.cast::<u8>())
            .filter(|_| status == 0)
            .ok_or_else(|| failed("the block buffer's bytes", status))?;
        // SAFETY: the block is one memory block of `len` bytes, which this slice alone writes
        // until the sample takes the block.
        fill(unsafe { slice::from_raw_parts_mut(bytes.as_ptr(), len) });
        let timing = CMSampleTimingInfo {
            duration: invalid_time(),
            presentationTimeStamp: CMTime {
                value: i64::try_from(pts_us).unwrap_or(i64::MAX),
                timescale: TIMESCALE,
                flags: CMTimeFlags::Valid,
                epoch: 0,
            },
            decodeTimeStamp: invalid_time(),
        };
        let mut sample = ptr::null_mut();
        // SAFETY: the pointers name values that live through the call, in the counts given.
        let status = unsafe {
            CMSampleBuffer::create_ready(
                None,
                Some(&block),
                Some(&self.description),
                1,
                1,
                &raw const timing,
                1,
                &raw const len,
                NonNull::from(&mut sample),
            )
        };
        NonNull::new(sample)
            .filter(|_| status == 0)
            // SAFETY: the function returns a sample that the caller owns, in the output.
            .map(|sample| unsafe { CFRetained::from_raw(sample) })
            .ok_or_else(|| failed("the sample buffer", status))
    }
}

fn invalid_time() -> CMTime {
    // SAFETY: a constant of CoreMedia
    unsafe { kCMTimeInvalid }
}

// A session that decodes the samples of one format. It tells the decoder's output callback of
// each picture.
pub(super) struct Session {
    raw: CFRetained<VTDecompressionSession>,
}

// SAFETY: VideoToolbox serializes the calls on a session, and CoreFoundation changes its
// reference count atomically. A session moves between the threads that feed it.
unsafe impl Send for Session {}

impl Session {
    // Makes a session for `format` that calls `callback` with `refcon` for each picture. The
    // session ends before it returns from drop, and the callback does not run after that, so
    // `refcon` needs to outlive the session only.
    pub(super) fn new(
        format: &Format,
        callback: VTDecompressionOutputCallbackRecord,
        order: DecodeOrder,
    ) -> Result<Self, DecoderError> {
        let attributes = destination_attributes();
        let specification = decoder_specification();
        let mut raw = ptr::null_mut();
        // SAFETY: the format and the dictionaries are valid objects of the types that the
        // function takes. The callback record and the output are valid pointers.
        let status = unsafe {
            VTDecompressionSession::create(
                None,
                &format.description,
                specification.as_deref(),
                Some(&attributes),
                &raw const callback,
                NonNull::from(&mut raw),
            )
        };
        let raw = NonNull::new(raw)
            .filter(|_| status == 0)
            // SAFETY: the function returns a session that the caller owns, in the output.
            .map(|raw| unsafe { CFRetained::from_raw(raw) })
            .ok_or_else(|| {
                DecoderError::Configure(format!("the decompression session: {status}"))
            })?;
        // Low-latency mode asks the hardware for immediate output; presentation mode allows
        // B-frame reference reordering without real-time throttling.
        let real_time_flag = order == DecodeOrder::LowLatency;
        // SAFETY: the key is a constant of VideoToolbox, and the value is a CFBoolean.
        let real_time = unsafe {
            objc2_video_toolbox::VTSessionSetProperty(
                &raw,
                kVTDecompressionPropertyKey_RealTime,
                Some(CFBoolean::new(real_time_flag).as_ref()),
            )
        };
        if real_time != 0 {
            log::debug!("video decoder: real-time property {real_time_flag}: {real_time}");
        }
        Ok(Self { raw })
    }

    // Whether the session decodes the pictures of `format` as well.
    pub(super) fn can_accept(&self, format: &Format) -> bool {
        // SAFETY: both are valid objects of the types that the function takes.
        unsafe { self.raw.can_accept_format_description(&format.description) }
    }

    // Queues `sample` for decoding and returns at once. The status is VideoToolbox's.
    pub(super) fn decode(&self, sample: &CMSampleBuffer, order: DecodeOrder) -> i32 {
        let flags = match order {
            DecodeOrder::LowLatency => VTDecodeFrameFlags::Frame_EnableAsynchronousDecompression,
            DecodeOrder::Presentation => {
                VTDecodeFrameFlags::Frame_EnableAsynchronousDecompression
                    | VTDecodeFrameFlags::Frame_EnableTemporalProcessing
            }
        };
        // SAFETY: the session and the sample are valid, and the other values are null or
        // flags. The picture comes back through the callback.
        unsafe {
            self.raw
                .decode_frame(sample, flags, ptr::null_mut(), ptr::null_mut())
        }
    }

    // Waits until every queued asynchronous decode callback has returned.
    pub(super) fn wait_for_asynchronous_frames(&self) {
        // SAFETY: the session is valid while this struct lives.
        unsafe {
            self.raw.wait_for_asynchronous_frames();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the session is valid. After the invalidation no callback runs.
        unsafe {
            self.raw.wait_for_asynchronous_frames();
            self.raw.invalidate();
        }
    }
}

// What the decoder makes its pictures in: IOSurface-backed buffers that Metal reads.
fn destination_attributes() -> CFRetained<CFDictionary> {
    let formats: Vec<CFRetained<CFNumber>> = OUTPUT_FORMATS
        .iter()
        .map(|format| CFNumber::new_i32(i32::try_from(*format).unwrap_or(0)))
        .collect();
    let formats =
        CFArray::<CFNumber>::from_objects(&formats.iter().map(|n| &**n).collect::<Vec<_>>());
    let surface_properties = CFDictionary::<CFString, CFType>::empty();
    // SAFETY: the keys are constants of CoreVideo.
    let (format_key, surface_key, metal_key) = unsafe {
        (
            kCVPixelBufferPixelFormatTypeKey,
            kCVPixelBufferIOSurfacePropertiesKey,
            kCVPixelBufferMetalCompatibilityKey,
        )
    };
    let dictionary = CFDictionary::<CFString, CFType>::from_slices(
        &[format_key, surface_key, metal_key],
        &[
            formats.as_ref(),
            surface_properties.as_ref(),
            CFBoolean::new(true).as_ref(),
        ],
    );
    // SAFETY: the dictionary's generics are not read through the untyped dictionary.
    unsafe { CFRetained::cast_unchecked::<CFDictionary>(dictionary) }
}

// A Mac asks for its hardware decoder, which iOS always uses.
#[cfg(target_os = "macos")]
fn decoder_specification() -> Option<CFRetained<CFDictionary>> {
    // SAFETY: the key is a constant of VideoToolbox.
    let key = unsafe { kVTVideoDecoderSpecification_EnableHardwareAcceleratedVideoDecoder };
    let dictionary =
        CFDictionary::<CFString, CFType>::from_slices(&[key], &[CFBoolean::new(true).as_ref()]);
    // SAFETY: as in `destination_attributes`
    Some(unsafe { CFRetained::cast_unchecked::<CFDictionary>(dictionary) })
}

#[cfg(not(target_os = "macos"))]
fn decoder_specification() -> Option<CFRetained<CFDictionary>> {
    None
}
