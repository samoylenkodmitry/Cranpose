//! Media Foundation `IMFTransform` setup, sample construction, and media-type metadata queries.
#![expect(
    unsafe_code,
    reason = "Media Foundation transforms, media types, and sample buffers are Win32 COM APIs"
)]

use std::ptr;

use windows::{
    Win32::{
        Media::MediaFoundation::{
            CODECAPI_AVLowLatencyMode, ICodecAPI, IMFDXGIDeviceManager, IMFMediaEventGenerator,
            IMFMediaType, IMFSample, IMFTransform, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
            MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
            MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
            MF_MT_YUV_MATRIX, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK, MFCreateMediaType,
            MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFNominalRange_0_255,
            MFNominalRange_16_235, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_SET_D3D_MANAGER,
            MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFVideoArea,
            MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoInterlace_Progressive,
            MFVideoPrimaries_BT470_2_SysBG, MFVideoPrimaries_BT709, MFVideoPrimaries_BT2020,
            MFVideoPrimaries_SMPTE170M, MFVideoTransFunc_709, MFVideoTransFunc_2020,
            MFVideoTransFunc_2084, MFVideoTransFunc_HLG, MFVideoTransFunc_sRGB,
            MFVideoTransferMatrix_BT601, MFVideoTransferMatrix_BT709,
            MFVideoTransferMatrix_BT2020_10, MFVideoTransferMatrix_BT2020_12,
        },
        System::Variant::VARIANT,
    },
    core::Interface,
};

use super::{
    ColorPrimaries, ColorRange, ColorTransfer, CropRect, DecodeOrder, DecoderError, VideoCodec,
    d3d11_frame::DxgiPixelFormat,
    media_foundation_capabilities::{codec_subtype, with_decoder_activates},
};

#[derive(Clone, Copy)]
pub(super) struct OutputFormat {
    pub(super) size: (u32, u32),
    pub(super) crop: CropRect,
    pub(super) pixel_format: DxgiPixelFormat,
    pub(super) primaries: Option<ColorPrimaries>,
    pub(super) range: Option<ColorRange>,
    pub(super) transfer: Option<ColorTransfer>,
}

pub(super) fn create_decoder_mft(codec: VideoCodec) -> Result<IMFTransform, DecoderError> {
    let hw_flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;
    if let Some(mft) = activate_first_mft(codec, hw_flags) {
        return Ok(mft);
    }
    let sw_flags = MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER;
    activate_first_mft(codec, sw_flags).ok_or(DecoderError::NoDecoder(codec))
}

fn activate_first_mft(codec: VideoCodec, flags: MFT_ENUM_FLAG) -> Option<IMFTransform> {
    with_decoder_activates(codec, flags, |slice| {
        for entry in slice {
            if let Some(activate) = entry.as_ref() {
                // SAFETY: instantiates the decoder COM transform.
                if let Ok(mft) = unsafe { activate.ActivateObject::<IMFTransform>() } {
                    return Some(mft);
                }
            }
        }
        None
    })
}

pub(super) fn unlock_async_if_needed(
    transform: &IMFTransform,
) -> Result<Option<IMFMediaEventGenerator>, DecoderError> {
    // SAFETY: queries transform attributes for `MF_TRANSFORM_ASYNC`.
    let Ok(attrs) = (unsafe { transform.GetAttributes() }) else {
        return Ok(None);
    };
    // SAFETY: reads `MF_TRANSFORM_ASYNC` flag.
    let is_async = unsafe { attrs.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
    if !is_async {
        return Ok(None);
    }
    // SAFETY: unlocks async MFT before any type/message calls per MSDN contract.
    unsafe { attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
        .map_err(|error| DecoderError::Failed(format!("unlock async MFT: {error}")))?;
    let generator: IMFMediaEventGenerator = transform
        .cast()
        .map_err(|error| DecoderError::Failed(format!("IMFMediaEventGenerator: {error}")))?;
    Ok(Some(generator))
}

pub(super) fn configure_low_latency(transform: &IMFTransform, order: DecodeOrder) {
    let Ok(codec_api) = transform.cast::<ICodecAPI>() else {
        return;
    };
    let low_latency = VARIANT::from(order == DecodeOrder::LowLatency);
    // SAFETY: sets optional `CODECAPI_AVLowLatencyMode` property.
    let _ = unsafe { codec_api.SetValue(&CODECAPI_AVLowLatencyMode, &low_latency) };
}

pub(super) fn attach_d3d_manager(transform: &IMFTransform, manager: &IMFDXGIDeviceManager) {
    // SAFETY: passes the `IMFDXGIDeviceManager` pointer to enable DXVA/D3D11 hardware decode.
    let _ =
        unsafe { transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize) };
}

pub(super) fn configure_input_type(
    transform: &IMFTransform,
    codec: VideoCodec,
    size: (u32, u32),
) -> Result<(), DecoderError> {
    // SAFETY: allocates a new Media Foundation media type descriptor.
    let media_type = unsafe { MFCreateMediaType() }
        .map_err(|error| DecoderError::Configure(format!("MFCreateMediaType: {error}")))?;
    let packed_size = (u64::from(size.0) << 32) | u64::from(size.1);
    // SAFETY: sets major type, codec subtype, frame size, and progressive interlace mode.
    unsafe {
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|()| media_type.SetGUID(&MF_MT_SUBTYPE, &codec_subtype(codec)))
            .and_then(|()| media_type.SetUINT64(&MF_MT_FRAME_SIZE, packed_size))
            .and_then(|()| {
                media_type.SetUINT32(
                    &MF_MT_INTERLACE_MODE,
                    u32::try_from(MFVideoInterlace_Progressive.0).unwrap_or(2),
                )
            })
            .and_then(|()| transform.SetInputType(0, &media_type, 0))
    }
    .map_err(|error| DecoderError::Configure(format!("SetInputType: {error}")))
}

// Selects the first available output type matching the decoder's native bit depth: MFTs list
// `P010` before `NV12` for 10-bit streams and `NV12` first for 8-bit streams.
pub(super) fn configure_output_type(
    transform: &IMFTransform,
    fallback_size: (u32, u32),
) -> Result<OutputFormat, DecoderError> {
    let mut selected: Option<(IMFMediaType, DxgiPixelFormat)> = None;
    for index in 0..32u32 {
        // SAFETY: enumerates candidate output types in MFT preference order.
        let Ok(candidate) = (unsafe { transform.GetOutputAvailableType(0, index) }) else {
            break;
        };
        // SAFETY: reads output subtype GUID.
        let Ok(subtype) = (unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) }) else {
            continue;
        };
        if subtype == MFVideoFormat_P010 {
            selected = Some((candidate, DxgiPixelFormat::P010));
            break;
        }
        if subtype == MFVideoFormat_NV12 {
            selected = Some((candidate, DxgiPixelFormat::Nv12));
            break;
        }
    }
    let (chosen, pixel_format) = selected
        .ok_or_else(|| DecoderError::Configure("MFT offers neither NV12 nor P010 output".into()))?;
    // SAFETY: applies the chosen output media type to stream 0.
    unsafe { transform.SetOutputType(0, &chosen, 0) }
        .map_err(|error| DecoderError::Configure(format!("SetOutputType: {error}")))?;
    // SAFETY: reads back the active output type for aperture and color metadata.
    let current = unsafe { transform.GetOutputCurrentType(0) }.unwrap_or(chosen);
    Ok(read_output_format(&current, fallback_size, pixel_format))
}

fn read_output_format(
    media_type: &IMFMediaType,
    fallback_size: (u32, u32),
    pixel_format: DxgiPixelFormat,
) -> OutputFormat {
    // SAFETY: reads packed `(width << 32) | height` attribute if present.
    let size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }
        .ok()
        .map(|packed| {
            (
                u32::try_from(packed >> 32).unwrap_or(0),
                u32::try_from(packed & 0xFFFF_FFFF).unwrap_or(0),
            )
        })
        .filter(|&(w, h)| w > 0 && h > 0)
        .unwrap_or(fallback_size);
    let crop = read_aperture(media_type).unwrap_or(CropRect {
        x: 0,
        y: 0,
        width: size.0,
        height: size.1,
    });
    OutputFormat {
        size,
        crop,
        pixel_format,
        primaries: read_mf_primaries(media_type),
        range: read_mf_range(media_type),
        transfer: read_mf_transfer(media_type),
    }
}

fn read_aperture(media_type: &IMFMediaType) -> Option<CropRect> {
    let mut area = MFVideoArea::default();
    // SAFETY: `MF_MT_MINIMUM_DISPLAY_APERTURE` is a fixed-size `MFVideoArea` POD struct.
    unsafe {
        let area_bytes = std::slice::from_raw_parts_mut(
            ptr::from_mut(&mut area).cast::<u8>(),
            size_of::<MFVideoArea>(),
        );
        media_type
            .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, area_bytes, None)
            .ok()?;
    }
    let x = u32::try_from(area.OffsetX.value).ok()?;
    let y = u32::try_from(area.OffsetY.value).ok()?;
    let width = u32::try_from(area.Area.cx).ok()?;
    let height = u32::try_from(area.Area.cy).ok()?;
    (width > 0 && height > 0).then_some(CropRect {
        x,
        y,
        width,
        height,
    })
}

fn read_mf_primaries(media_type: &IMFMediaType) -> Option<ColorPrimaries> {
    // SAFETY: reads optional `MF_MT_YUV_MATRIX` / `MF_MT_VIDEO_PRIMARIES` enum values.
    if let Ok(matrix) = unsafe { media_type.GetUINT32(&MF_MT_YUV_MATRIX) } {
        let matrix = i32::try_from(matrix).ok()?;
        if matrix == MFVideoTransferMatrix_BT709.0 {
            return Some(ColorPrimaries::Bt709);
        }
        if matrix == MFVideoTransferMatrix_BT601.0 {
            return Some(ColorPrimaries::Bt601);
        }
        if matrix == MFVideoTransferMatrix_BT2020_10.0
            || matrix == MFVideoTransferMatrix_BT2020_12.0
        {
            return Some(ColorPrimaries::Bt2020);
        }
    }
    let primaries =
        i32::try_from(unsafe { media_type.GetUINT32(&MF_MT_VIDEO_PRIMARIES) }.ok()?).ok()?;
    if primaries == MFVideoPrimaries_BT709.0 {
        Some(ColorPrimaries::Bt709)
    } else if primaries == MFVideoPrimaries_SMPTE170M.0
        || primaries == MFVideoPrimaries_BT470_2_SysBG.0
    {
        Some(ColorPrimaries::Bt601)
    } else if primaries == MFVideoPrimaries_BT2020.0 {
        Some(ColorPrimaries::Bt2020)
    } else {
        None
    }
}

fn read_mf_range(media_type: &IMFMediaType) -> Option<ColorRange> {
    // SAFETY: reads optional `MF_MT_VIDEO_NOMINAL_RANGE` enum value.
    let range =
        i32::try_from(unsafe { media_type.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) }.ok()?).ok()?;
    if range == MFNominalRange_0_255.0 {
        Some(ColorRange::Full)
    } else if range == MFNominalRange_16_235.0 {
        Some(ColorRange::Limited)
    } else {
        None
    }
}

fn read_mf_transfer(media_type: &IMFMediaType) -> Option<ColorTransfer> {
    // SAFETY: reads optional `MF_MT_TRANSFER_FUNCTION` enum value.
    let transfer =
        i32::try_from(unsafe { media_type.GetUINT32(&MF_MT_TRANSFER_FUNCTION) }.ok()?).ok()?;
    if transfer == MFVideoTransFunc_2084.0 {
        Some(ColorTransfer::Pq)
    } else if transfer == MFVideoTransFunc_HLG.0 {
        Some(ColorTransfer::Hlg)
    } else if transfer == MFVideoTransFunc_709.0
        || transfer == MFVideoTransFunc_sRGB.0
        || transfer == MFVideoTransFunc_2020.0
    {
        Some(ColorTransfer::Sdr)
    } else {
        None
    }
}

pub(super) fn query_output_stream_info(
    transform: &IMFTransform,
) -> Result<(bool, u32), DecoderError> {
    // SAFETY: queries stream 0 output buffer allocation requirements.
    let info = unsafe { transform.GetOutputStreamInfo(0) }
        .map_err(|error| DecoderError::Configure(format!("GetOutputStreamInfo: {error}")))?;
    let provides_flags = u32::try_from(
        MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0,
    )
    .unwrap_or(0);
    Ok(((info.dwFlags & provides_flags) != 0, info.cbSize))
}

pub(super) fn create_input_sample(data: &[u8], pts_us: u64) -> Result<IMFSample, DecoderError> {
    let len = u32::try_from(data.len()).map_err(|_| DecoderError::AccessUnitTooLarge {
        size: data.len(),
        limit: u32::MAX as usize,
    })?;
    // SAFETY: creates a Media Foundation sample and memory buffer and copies the access unit in.
    unsafe {
        let sample = MFCreateSample()
            .map_err(|error| DecoderError::Failed(format!("MFCreateSample: {error}")))?;
        let buffer = MFCreateMemoryBuffer(len)
            .map_err(|error| DecoderError::Failed(format!("MFCreateMemoryBuffer: {error}")))?;
        let mut dst = ptr::null_mut();
        buffer
            .Lock(&mut dst, None, None)
            .map_err(|error| DecoderError::Failed(format!("buffer Lock: {error}")))?;
        ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
        let _ = buffer.Unlock();
        buffer
            .SetCurrentLength(len)
            .map_err(|error| DecoderError::Failed(format!("SetCurrentLength: {error}")))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|error| DecoderError::Failed(format!("AddBuffer: {error}")))?;
        let pts_100ns = i64::try_from(pts_us.saturating_mul(10)).unwrap_or(i64::MAX);
        let _ = sample.SetSampleTime(pts_100ns);
        Ok(sample)
    }
}

pub(super) fn create_output_sample(size: u32) -> Result<IMFSample, DecoderError> {
    // SAFETY: allocates an output sample for synchronous software MFTs that do not allocate their own.
    unsafe {
        let sample = MFCreateSample()
            .map_err(|error| DecoderError::Failed(format!("MFCreateSample: {error}")))?;
        let buffer = MFCreateMemoryBuffer(size.max(4096))
            .map_err(|error| DecoderError::Failed(format!("MFCreateMemoryBuffer: {error}")))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|error| DecoderError::Failed(format!("AddBuffer: {error}")))?;
        Ok(sample)
    }
}

pub(super) fn clamp_crop(crop: CropRect, size: (u32, u32)) -> CropRect {
    if crop.width > 0
        && crop.height > 0
        && crop.x.checked_add(crop.width).is_some_and(|r| r <= size.0)
        && crop.y.checked_add(crop.height).is_some_and(|b| b <= size.1)
    {
        crop
    } else {
        CropRect {
            x: 0,
            y: 0,
            width: size.0,
            height: size.1,
        }
    }
}
