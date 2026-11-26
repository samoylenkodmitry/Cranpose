//! Hardware video decoder capability probing on Windows through Media Foundation `MFTEnumEx`.
#![expect(
    unsafe_code,
    reason = "Media Foundation MFT enumeration uses Win32 COM pointers and CoTaskMemFree"
)]

use std::ptr;

use windows::{
    Win32::{
        Media::MediaFoundation::{
            IMFActivate, MF_VERSION, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup,
            MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ASYNCMFT,
            MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
            MFT_FRIENDLY_NAME_Attribute, MFT_OUTPUT_TYPES_Attributes, MFT_REGISTER_TYPE_INFO,
            MFTEnumEx, MFVideoFormat_AV1, MFVideoFormat_H264, MFVideoFormat_HEVC,
            MFVideoFormat_P010, MFVideoFormat_VP90,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    },
    core::{GUID, PWSTR},
};

use super::{DecoderCapabilities, DecoderCapabilitySource, VideoCodec};

pub(super) struct Limits;

impl DecoderCapabilitySource for Limits {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        let _mf = MfRuntimeGuard::enter()?;
        let hw_flags = MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER;
        if let Some(info) = probe_first(codec, hw_flags) {
            return Some(DecoderCapabilities {
                codec,
                name: info.name,
                hardware_accelerated: Some(true),
                max_width: None,
                max_height: None,
                max_bitrate: None,
                b_frame_reordering: true,
                hdr_10bit: Some(info.supports_p010),
                zero_copy_gpu: true,
            });
        }
        let sw_flags = MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER;
        let info = probe_first(codec, sw_flags)?;
        Some(DecoderCapabilities {
            codec,
            name: info.name,
            hardware_accelerated: Some(false),
            max_width: None,
            max_height: None,
            max_bitrate: None,
            b_frame_reordering: true,
            hdr_10bit: Some(info.supports_p010),
            zero_copy_gpu: false,
        })
    }

    fn max_frame_rate(&self, _codec: VideoCodec, _width: u32, _height: u32) -> Option<f32> {
        None
    }
}

pub(super) fn codec_subtype(codec: VideoCodec) -> GUID {
    match codec {
        VideoCodec::H264 => MFVideoFormat_H264,
        VideoCodec::Hevc => MFVideoFormat_HEVC,
        VideoCodec::Vp9 => MFVideoFormat_VP90,
        VideoCodec::Av1 => MFVideoFormat_AV1,
    }
}

struct ProbedMft {
    name: String,
    supports_p010: bool,
}

pub(super) fn with_decoder_activates<T>(
    codec: VideoCodec,
    flags: MFT_ENUM_FLAG,
    visit: impl FnOnce(&[Option<IMFActivate>]) -> Option<T>,
) -> Option<T> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec_subtype(codec),
    };
    let mut activates: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: `MFTEnumEx` allocates a COM task-memory array of `IMFActivate` pointers freed below.
    let status = unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&input),
            None,
            &mut activates,
            &mut count,
        )
    };
    if status.is_err() || activates.is_null() || count == 0 {
        if !activates.is_null() {
            // SAFETY: `activates` was returned by `MFTEnumEx`.
            unsafe { CoTaskMemFree(Some(activates.cast())) };
        }
        return None;
    }
    // SAFETY: `activates` holds `count` elements allocated by `MFTEnumEx`.
    let slice = unsafe { std::slice::from_raw_parts_mut(activates, count as usize) };
    let result = visit(slice);
    for entry in slice {
        let _ = entry.take();
    }
    // SAFETY: releases the COM array allocated by `MFTEnumEx`.
    unsafe { CoTaskMemFree(Some(activates.cast())) };
    result
}

fn probe_first(codec: VideoCodec, flags: MFT_ENUM_FLAG) -> Option<ProbedMft> {
    with_decoder_activates(codec, flags, |slice| {
        let activate = slice.first()?.as_ref()?;
        Some(ProbedMft {
            name: read_friendly_name(activate).unwrap_or_else(|| format!("MF {}", codec.mime())),
            supports_p010: advertises_p010(activate),
        })
    })
}

fn read_friendly_name(activate: &IMFActivate) -> Option<String> {
    let mut wide = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: `GetAllocatedString` writes a task-memory UTF-16 buffer freed immediately below.
    unsafe { activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut wide, &mut len) }
        .ok()?;
    if wide.is_null() {
        return None;
    }
    // SAFETY: `wide` is valid for `len` UTF-16 code units as reported by Media Foundation.
    let slice = unsafe { std::slice::from_raw_parts(wide.0, len as usize) };
    let name = String::from_utf16_lossy(slice);
    // SAFETY: frees the string allocated by `GetAllocatedString`.
    unsafe { CoTaskMemFree(Some(wide.0.cast())) };
    Some(name)
}

fn advertises_p010(activate: &IMFActivate) -> bool {
    let mut ptr_buf: *mut u8 = ptr::null_mut();
    let mut byte_len = 0u32;
    // SAFETY: `GetAllocatedBlob` allocates an array of `MFT_REGISTER_TYPE_INFO` structs.
    let ok = unsafe {
        activate.GetAllocatedBlob(&MFT_OUTPUT_TYPES_Attributes, &mut ptr_buf, &mut byte_len)
    }
    .is_ok();
    if !ok || ptr_buf.is_null() {
        return false;
    }
    let entry_size = size_of::<MFT_REGISTER_TYPE_INFO>();
    let count = (byte_len as usize) / entry_size;
    let mut found = false;
    for i in 0..count {
        // SAFETY: `ptr_buf` holds `count` `MFT_REGISTER_TYPE_INFO` values; `read_unaligned`
        // handles any allocator alignment.
        let info = unsafe {
            ptr::read_unaligned(ptr_buf.add(i * entry_size).cast::<MFT_REGISTER_TYPE_INFO>())
        };
        if info.guidSubtype == MFVideoFormat_P010 {
            found = true;
            break;
        }
    }
    // SAFETY: frees the blob allocated by `GetAllocatedBlob`.
    unsafe { CoTaskMemFree(Some(ptr_buf.cast())) };
    found
}

pub(super) struct MfRuntimeGuard {
    com_initialized: bool,
}

impl MfRuntimeGuard {
    pub(super) fn enter() -> Option<Self> {
        // SAFETY: initializes multithreaded COM for this thread; `RPC_E_CHANGED_MODE` is fine.
        let com_hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = com_hr.is_ok();
        // SAFETY: balances with `MFShutdown` in `Drop`.
        if unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }.is_err() {
            if com_initialized {
                // SAFETY: matches our `CoInitializeEx` above.
                unsafe { CoUninitialize() };
            }
            return None;
        }
        Some(Self { com_initialized })
    }
}

impl Drop for MfRuntimeGuard {
    fn drop(&mut self) {
        // SAFETY: balances `MFStartup` in `enter`.
        let _ = unsafe { MFShutdown() };
        if self.com_initialized {
            // SAFETY: balances `CoInitializeEx` in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}
