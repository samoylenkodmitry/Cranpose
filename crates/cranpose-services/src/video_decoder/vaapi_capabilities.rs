//! Hardware VA-API and software fallback capability probing on Linux.
#![expect(
    unsafe_code,
    reason = "VA-API display and profile queries call loaded libva C functions"
)]

use std::{
    ffi::{CStr, c_int},
    sync::OnceLock,
};

use super::{
    DecoderCapabilities, DecoderCapabilitySource, VideoCodec,
    vaapi_loader::{
        VA_ENTRY_VLD, VA_PROFILE_AV1_PROFILE0, VA_PROFILE_H264_CONSTRAINED_BASELINE,
        VA_PROFILE_H264_HIGH, VA_PROFILE_H264_MAIN, VA_PROFILE_HEVC_MAIN, VA_PROFILE_HEVC_MAIN10,
        VA_PROFILE_VP9_PROFILE0, VA_PROFILE_VP9_PROFILE2, VA_STATUS_SUCCESS, VaApi, VaDisplay,
        av_api, render_nodes, va_api,
    },
};

pub(super) struct Limits;

#[derive(Clone, Copy, Debug)]
pub(super) struct VaApiSupport {
    pub(super) vld: bool,
    pub(super) hdr_10bit: bool,
}

#[derive(Clone, Copy)]
struct CachedVaSupport {
    h264: VaApiSupport,
    hevc: VaApiSupport,
    vp9: VaApiSupport,
    av1: VaApiSupport,
}

static VA_SUPPORT: OnceLock<Option<CachedVaSupport>> = OnceLock::new();

impl DecoderCapabilitySource for Limits {
    fn capabilities(&self, codec: VideoCodec) -> Option<DecoderCapabilities> {
        let av = av_api()?;
        let decoder_name = find_av_decoder_name(av, codec)?;
        if let Some(va) = probe_vaapi_codec(codec)
            && va.vld
        {
            return Some(DecoderCapabilities {
                codec,
                name: format!("vaapi/{decoder_name}"),
                hardware_accelerated: Some(true),
                max_width: None,
                max_height: None,
                max_bitrate: None,
                b_frame_reordering: true,
                hdr_10bit: Some(va.hdr_10bit),
                zero_copy_gpu: true,
            });
        }
        Some(DecoderCapabilities {
            codec,
            name: format!("software/{decoder_name}"),
            hardware_accelerated: Some(false),
            max_width: None,
            max_height: None,
            max_bitrate: None,
            b_frame_reordering: true,
            hdr_10bit: Some(codec != VideoCodec::H264),
            zero_copy_gpu: false,
        })
    }

    fn max_frame_rate(&self, _codec: VideoCodec, _width: u32, _height: u32) -> Option<f32> {
        None
    }
}

pub(super) fn av_decoder_names(codec: VideoCodec) -> &'static [&'static CStr] {
    match codec {
        VideoCodec::H264 => &[c"h264"],
        VideoCodec::Hevc => &[c"hevc"],
        VideoCodec::Vp9 => &[c"vp9"],
        VideoCodec::Av1 => &[c"av1", c"libdav1d"],
    }
}

pub(super) fn find_av_decoder_name(
    av: &super::vaapi_loader::AvApi,
    codec: VideoCodec,
) -> Option<&'static str> {
    for &name in av_decoder_names(codec) {
        // SAFETY: `name` is a static NUL-terminated FFmpeg decoder name.
        if !unsafe { (av.avcodec_find_decoder_by_name)(name.as_ptr()) }.is_null() {
            return name.to_str().ok();
        }
    }
    None
}

pub(super) fn probe_vaapi_codec(codec: VideoCodec) -> Option<VaApiSupport> {
    let cached = (*VA_SUPPORT.get_or_init(probe_all_vaapi_codecs))?;
    Some(match codec {
        VideoCodec::H264 => cached.h264,
        VideoCodec::Hevc => cached.hevc,
        VideoCodec::Vp9 => cached.vp9,
        VideoCodec::Av1 => cached.av1,
    })
}

fn probe_all_vaapi_codecs() -> Option<CachedVaSupport> {
    let va = va_api()?;
    for node in render_nodes() {
        // SAFETY: opens DRM render node read-write for one-shot VA-API probe and closes it.
        let fd = unsafe { libc::open(node.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            continue;
        }
        let support = query_display_support(va, fd);
        // SAFETY: closes the DRM render node FD opened above.
        unsafe { libc::close(fd) };
        if let Some(support) = support {
            return Some(support);
        }
    }
    None
}

fn query_display_support(va: &VaApi, fd: c_int) -> Option<CachedVaSupport> {
    // SAFETY: initializes a VA-API DRM display and terminates it before returning.
    unsafe {
        let display = (va.get_display_drm)(fd);
        if display.is_null() {
            return None;
        }
        let mut major = 0;
        let mut minor = 0;
        if (va.initialize)(display, &mut major, &mut minor) != VA_STATUS_SUCCESS {
            return None;
        }
        let support = query_all_profiles(va, display);
        let _ = (va.terminate)(display);
        Some(support)
    }
}

unsafe fn query_all_profiles(va: &VaApi, display: VaDisplay) -> CachedVaSupport {
    let empty = VaApiSupport {
        vld: false,
        hdr_10bit: false,
    };
    // SAFETY: queries VA-API profile list and VLD entrypoints on the initialized display.
    let max_profiles = usize::try_from(unsafe { (va.max_num_profiles)(display) }).unwrap_or(0);
    if max_profiles == 0 {
        return CachedVaSupport {
            h264: empty,
            hevc: empty,
            vp9: empty,
            av1: empty,
        };
    }
    let mut profiles = vec![0 as c_int; max_profiles];
    let mut count: c_int = 0;
    if unsafe { (va.query_config_profiles)(display, profiles.as_mut_ptr(), &mut count) }
        != VA_STATUS_SUCCESS
    {
        return CachedVaSupport {
            h264: empty,
            hevc: empty,
            vp9: empty,
            av1: empty,
        };
    }
    profiles.truncate(usize::try_from(count).unwrap_or(0));
    let has_vld = |profile: c_int| -> bool {
        profiles.contains(&profile) && unsafe { supports_vld(va, display, profile) }
    };
    let hevc_main = has_vld(VA_PROFILE_HEVC_MAIN);
    let hevc_main10 = has_vld(VA_PROFILE_HEVC_MAIN10);
    let vp9_p0 = has_vld(VA_PROFILE_VP9_PROFILE0);
    let vp9_p2 = has_vld(VA_PROFILE_VP9_PROFILE2);
    let av1_p0 = has_vld(VA_PROFILE_AV1_PROFILE0);
    CachedVaSupport {
        h264: VaApiSupport {
            vld: has_vld(VA_PROFILE_H264_HIGH)
                || has_vld(VA_PROFILE_H264_MAIN)
                || has_vld(VA_PROFILE_H264_CONSTRAINED_BASELINE),
            hdr_10bit: false,
        },
        hevc: VaApiSupport {
            vld: hevc_main || hevc_main10,
            hdr_10bit: hevc_main10,
        },
        vp9: VaApiSupport {
            vld: vp9_p0 || vp9_p2,
            hdr_10bit: vp9_p2,
        },
        av1: VaApiSupport {
            vld: av1_p0,
            hdr_10bit: av1_p0,
        },
    }
}

unsafe fn supports_vld(va: &VaApi, display: VaDisplay, profile: c_int) -> bool {
    // SAFETY: queries supported VA-API entrypoints for `profile`.
    let max_eps = usize::try_from(unsafe { (va.max_num_entrypoints)(display) }).unwrap_or(0);
    if max_eps == 0 {
        return false;
    }
    let mut eps = vec![0 as c_int; max_eps];
    let mut count: c_int = 0;
    if unsafe { (va.query_config_entrypoints)(display, profile, eps.as_mut_ptr(), &mut count) }
        != VA_STATUS_SUCCESS
    {
        return false;
    }
    eps.truncate(usize::try_from(count).unwrap_or(0));
    eps.contains(&VA_ENTRY_VLD)
}
