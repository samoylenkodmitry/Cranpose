//! Runtime dynamic loader for Linux `libva`, `libva-drm`, `libavcodec`, and `libavutil`.
//!
//! Loading these libraries with `dlopen` avoids compile-time C header dependencies and probes
//! `AVCodecContext` / `AVFrame` field offsets at runtime via `av_opt_find` and sentinel matching,
//! keeping ABI compatibility across FFmpeg 5, 6, 7, 8, and 9 distributions.
#![expect(
    unsafe_code,
    reason = "dlopen/dlsym and C ABI function pointers for libva and libavcodec"
)]

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    fs,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    ptr,
    sync::{
        OnceLock,
        atomic::{AtomicI32, Ordering},
    },
};

pub(super) type VaDisplay = *mut c_void;
pub(super) type VaStatus = c_int;
pub(super) type VaSurfaceId = u32;

pub(super) const VA_STATUS_SUCCESS: VaStatus = 0;
pub(super) const VA_ENTRY_VLD: c_int = 1;
pub(super) const VA_PROFILE_H264_MAIN: c_int = 6;
pub(super) const VA_PROFILE_H264_HIGH: c_int = 7;
pub(super) const VA_PROFILE_H264_CONSTRAINED_BASELINE: c_int = 13;
pub(super) const VA_PROFILE_HEVC_MAIN: c_int = 17;
pub(super) const VA_PROFILE_HEVC_MAIN10: c_int = 18;
pub(super) const VA_PROFILE_VP9_PROFILE0: c_int = 19;
pub(super) const VA_PROFILE_VP9_PROFILE2: c_int = 21;
pub(super) const VA_PROFILE_AV1_PROFILE0: c_int = 32;

pub(super) const VA_SURFACE_ATTRIB_MEM_TYPE_DRM_PRIME_2: u32 = 0x4000_0000;
pub(super) const VA_EXPORT_SURFACE_READ_ONLY: u32 = 0x0001;
pub(super) const VA_EXPORT_SURFACE_SEPARATE_LAYERS: u32 = 0x0004;

pub(super) const VA_FOURCC_P010: u32 = u32::from_le_bytes(*b"P010");

pub(super) const AV_NOPTS_VALUE: i64 = i64::MIN;
pub(super) const AV_PIX_FMT_NONE: c_int = -1;
pub(super) const AV_CODEC_FLAG_LOW_DELAY: i64 = 1 << 19;
pub(super) const AVERROR_EOF: c_int = -(i32::from_le_bytes(*b"EOF "));

// Stable `libva` 2.x `VADRMPRIMESurfaceDescriptor` layout (`<va/va_drmcommon.h>`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct VaDrmPrimeObject {
    pub(super) fd: c_int,
    pub(super) size: u32,
    pub(super) drm_format_modifier: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct VaDrmPrimeLayer {
    pub(super) drm_format: u32,
    pub(super) num_planes: u32,
    pub(super) object_index: [u32; 4],
    pub(super) offset: [u32; 4],
    pub(super) pitch: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct VaDrmPrimeSurfaceDescriptor {
    pub(super) fourcc: u32,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) num_objects: u32,
    pub(super) objects: [VaDrmPrimeObject; 4],
    pub(super) num_layers: u32,
    pub(super) layers: [VaDrmPrimeLayer; 4],
}

#[repr(C)]
struct AvOption {
    name: *const c_char,
    help: *const c_char,
    offset: c_int,
}

#[repr(C)]
pub(super) struct AvBufferRef {
    _buffer: *mut c_void,
    pub(super) data: *mut u8,
}

#[repr(C)]
pub(super) struct AvHwDeviceContext {
    _av_class: *const c_void,
    _internal: *mut c_void,
    pub(super) hwctx: *mut c_void,
}

#[repr(C)]
pub(super) struct AvPacketHead {
    pub(super) buf: *mut c_void,
    pub(super) pts: i64,
    pub(super) dts: i64,
    pub(super) data: *mut u8,
    pub(super) size: c_int,
}

// First 120 bytes of `AVFrame`, ABI-stable across FFmpeg 5, 6, 7, 8, and 9.
#[repr(C)]
pub(super) struct AvFrameHead {
    pub(super) data: [*mut u8; 8],
    pub(super) linesize: [c_int; 8],
    pub(super) extended_data: *mut *mut u8,
    pub(super) width: c_int,
    pub(super) height: c_int,
    pub(super) nb_samples: c_int,
    pub(super) format: c_int,
}

pub(super) struct VaApi {
    _libva: *mut c_void,
    _libva_drm: *mut c_void,
    pub(super) get_display_drm: unsafe extern "C" fn(c_int) -> VaDisplay,
    pub(super) initialize: unsafe extern "C" fn(VaDisplay, *mut c_int, *mut c_int) -> VaStatus,
    pub(super) terminate: unsafe extern "C" fn(VaDisplay) -> VaStatus,
    pub(super) max_num_profiles: unsafe extern "C" fn(VaDisplay) -> c_int,
    pub(super) query_config_profiles:
        unsafe extern "C" fn(VaDisplay, *mut c_int, *mut c_int) -> VaStatus,
    pub(super) max_num_entrypoints: unsafe extern "C" fn(VaDisplay) -> c_int,
    pub(super) query_config_entrypoints:
        unsafe extern "C" fn(VaDisplay, c_int, *mut c_int, *mut c_int) -> VaStatus,
    pub(super) sync_surface: unsafe extern "C" fn(VaDisplay, VaSurfaceId) -> VaStatus,
    pub(super) export_surface_handle:
        unsafe extern "C" fn(VaDisplay, VaSurfaceId, u32, u32, *mut c_void) -> VaStatus,
}

// SAFETY: `VaApi` holds process-wide immutable dlopen handles and C function pointers.
unsafe impl Send for VaApi {}
// SAFETY: as for Send.
unsafe impl Sync for VaApi {}

#[derive(Clone, Copy)]
pub(super) struct AvOffsets {
    pub(super) get_format: usize,
    pub(super) hw_device_ctx: usize,
    pub(super) frame_pts: usize,
}

#[derive(Clone, Copy)]
pub(super) struct AvPixelFormats {
    pub(super) vaapi: c_int,
    pub(super) nv12: c_int,
    pub(super) p010le: c_int,
    pub(super) yuv420p: c_int,
    pub(super) yuvj420p: c_int,
    pub(super) yuv420p10le: c_int,
}

pub(super) struct AvApi {
    _libavutil: *mut c_void,
    _libavcodec: *mut c_void,
    pub(super) av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub(super) av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_frame_unref: unsafe extern "C" fn(*mut c_void),
    pub(super) av_frame_clone: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub(super) av_buffer_ref: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pub(super) av_buffer_unref: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_hwdevice_ctx_create:
        unsafe extern "C" fn(*mut *mut c_void, c_int, *const c_char, *mut c_void, c_int) -> c_int,
    pub(super) av_opt_set_int:
        unsafe extern "C" fn(*mut c_void, *const c_char, i64, c_int) -> c_int,
    pub(super) avcodec_find_decoder_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    pub(super) avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub(super) avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) avcodec_open2:
        unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    pub(super) avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub(super) avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub(super) avcodec_flush_buffers: unsafe extern "C" fn(*mut c_void),
    pub(super) avcodec_default_get_format: unsafe extern "C" fn(*mut c_void, *const c_int) -> c_int,
    pub(super) av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub(super) av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_new_packet: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    pub(super) av_packet_unref: unsafe extern "C" fn(*mut c_void),
    pub(super) vaapi_device_type: c_int,
    pub(super) pix_fmts: AvPixelFormats,
    pub(super) offsets: AvOffsets,
}

// SAFETY: `AvApi` holds process-wide immutable dlopen handles, function pointers, and constants.
unsafe impl Send for AvApi {}
// SAFETY: as for Send.
unsafe impl Sync for AvApi {}

static VA_API: OnceLock<Option<VaApi>> = OnceLock::new();
static AV_API: OnceLock<Option<AvApi>> = OnceLock::new();
static VAAPI_PIX_FMT_GLOBAL: AtomicI32 = AtomicI32::new(AV_PIX_FMT_NONE);

pub(super) fn va_api() -> Option<&'static VaApi> {
    VA_API.get_or_init(load_va_api).as_ref()
}

pub(super) fn av_api() -> Option<&'static AvApi> {
    AV_API.get_or_init(load_av_api).as_ref()
}

// `AVCodecContext::get_format` callback selecting `AV_PIX_FMT_VAAPI` when offered.
pub(super) unsafe extern "C" fn select_vaapi_format(
    ctx: *mut c_void,
    mut fmts: *const c_int,
) -> c_int {
    let wanted = VAAPI_PIX_FMT_GLOBAL.load(Ordering::Relaxed);
    if wanted != AV_PIX_FMT_NONE && !fmts.is_null() {
        // SAFETY: `fmts` is an `AV_PIX_FMT_NONE`-terminated array provided by `libavcodec`.
        while unsafe { *fmts } != AV_PIX_FMT_NONE {
            if unsafe { *fmts } == wanted {
                return wanted;
            }
            // SAFETY: advancing within the sentinel-terminated `fmts` list.
            fmts = unsafe { fmts.add(1) };
        }
    }
    if let Some(av) = av_api() {
        // SAFETY: falls back to libavcodec's default format selection.
        return unsafe { (av.avcodec_default_get_format)(ctx, fmts) };
    }
    AV_PIX_FMT_NONE
}

// Discovers DRM render nodes (`/dev/dri/renderD128`, ...) in ascending order.
pub(super) fn render_nodes() -> Vec<CString> {
    let mut nodes: Vec<PathBuf> = fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("renderD"))
        })
        .collect();
    nodes.sort();
    nodes
        .into_iter()
        .filter_map(|path| CString::new(path.as_os_str().as_bytes()).ok())
        .collect()
}

unsafe fn open_any(names: &[&CStr]) -> Option<*mut c_void> {
    for name in names {
        // SAFETY: `name` is a static NUL-terminated shared-object filename.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            return Some(handle);
        }
    }
    None
}

unsafe fn load_sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Option<T> {
    // SAFETY: `lib` is a valid dlopen handle and `name` is a static C symbol name.
    let ptr = unsafe { libc::dlsym(lib, name.as_ptr()) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller ensures `T` matches the C function pointer signature of `name`.
    Some(unsafe { std::mem::transmute_copy(&ptr) })
}

fn load_va_api() -> Option<VaApi> {
    // SAFETY: loads `libva.so.2` and `libva-drm.so.2` symbols with matching C signatures.
    unsafe {
        let libva = open_any(&[c"libva.so.2", c"libva.so"])?;
        let libva_drm = open_any(&[c"libva-drm.so.2", c"libva-drm.so"])?;
        Some(VaApi {
            _libva: libva,
            _libva_drm: libva_drm,
            get_display_drm: load_sym(libva_drm, c"vaGetDisplayDRM")?,
            initialize: load_sym(libva, c"vaInitialize")?,
            terminate: load_sym(libva, c"vaTerminate")?,
            max_num_profiles: load_sym(libva, c"vaMaxNumProfiles")?,
            query_config_profiles: load_sym(libva, c"vaQueryConfigProfiles")?,
            max_num_entrypoints: load_sym(libva, c"vaMaxNumEntrypoints")?,
            query_config_entrypoints: load_sym(libva, c"vaQueryConfigEntrypoints")?,
            sync_surface: load_sym(libva, c"vaSyncSurface")?,
            export_surface_handle: load_sym(libva, c"vaExportSurfaceHandle")?,
        })
    }
}

fn load_av_api() -> Option<AvApi> {
    // Pair matching FFmpeg SONAMEs so libavcodec and libavutil always belong to the same release.
    const PAIRS: [(&CStr, &CStr); 7] = [
        (c"libavcodec.so.63", c"libavutil.so.61"),
        (c"libavcodec.so.62", c"libavutil.so.60"),
        (c"libavcodec.so.61", c"libavutil.so.59"),
        (c"libavcodec.so.60", c"libavutil.so.58"),
        (c"libavcodec.so.59", c"libavutil.so.57"),
        (c"libavcodec.so.58", c"libavutil.so.56"),
        (c"libavcodec.so", c"libavutil.so"),
    ];
    for (codec_so, util_so) in PAIRS {
        // SAFETY: checks whether both libraries of one FFmpeg release are installed.
        let (codec_lib, util_lib) = unsafe {
            let Some(u) = open_any(&[util_so]) else {
                continue;
            };
            let Some(c) = open_any(&[codec_so]) else {
                libc::dlclose(u);
                continue;
            };
            (c, u)
        };
        if let Some(api) = load_av_symbols(codec_lib, util_lib) {
            return Some(api);
        }
        // SAFETY: releases handles if symbol resolution failed.
        unsafe {
            libc::dlclose(codec_lib);
            libc::dlclose(util_lib);
        }
    }
    None
}

struct AvUtilSyms {
    av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    av_frame_unref: unsafe extern "C" fn(*mut c_void),
    av_frame_clone: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    av_buffer_ref: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    av_buffer_unref: unsafe extern "C" fn(*mut *mut c_void),
    av_hwdevice_find_type_by_name: unsafe extern "C" fn(*const c_char) -> c_int,
    av_hwdevice_ctx_create:
        unsafe extern "C" fn(*mut *mut c_void, c_int, *const c_char, *mut c_void, c_int) -> c_int,
    av_get_pix_fmt: unsafe extern "C" fn(*const c_char) -> c_int,
    av_opt_find: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        c_int,
        c_int,
    ) -> *const AvOption,
    av_opt_set_int: unsafe extern "C" fn(*mut c_void, *const c_char, i64, c_int) -> c_int,
}

struct AvCodecSyms {
    avcodec_find_decoder_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    avcodec_open2: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    avcodec_flush_buffers: unsafe extern "C" fn(*mut c_void),
    avcodec_default_get_format: unsafe extern "C" fn(*mut c_void, *const c_int) -> c_int,
    av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    av_new_packet: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    av_packet_unref: unsafe extern "C" fn(*mut c_void),
}

unsafe fn load_avutil_syms(util_lib: *mut c_void) -> Option<AvUtilSyms> {
    // SAFETY: resolves standard libavutil C symbols with matching signatures.
    unsafe {
        Some(AvUtilSyms {
            av_frame_alloc: load_sym(util_lib, c"av_frame_alloc")?,
            av_frame_free: load_sym(util_lib, c"av_frame_free")?,
            av_frame_unref: load_sym(util_lib, c"av_frame_unref")?,
            av_frame_clone: load_sym(util_lib, c"av_frame_clone")?,
            av_buffer_ref: load_sym(util_lib, c"av_buffer_ref")?,
            av_buffer_unref: load_sym(util_lib, c"av_buffer_unref")?,
            av_hwdevice_find_type_by_name: load_sym(util_lib, c"av_hwdevice_find_type_by_name")?,
            av_hwdevice_ctx_create: load_sym(util_lib, c"av_hwdevice_ctx_create")?,
            av_get_pix_fmt: load_sym(util_lib, c"av_get_pix_fmt")?,
            av_opt_find: load_sym(util_lib, c"av_opt_find")?,
            av_opt_set_int: load_sym(util_lib, c"av_opt_set_int")?,
        })
    }
}

unsafe fn load_avcodec_syms(codec_lib: *mut c_void) -> Option<AvCodecSyms> {
    // SAFETY: resolves standard libavcodec C symbols with matching signatures.
    unsafe {
        Some(AvCodecSyms {
            avcodec_find_decoder_by_name: load_sym(codec_lib, c"avcodec_find_decoder_by_name")?,
            avcodec_alloc_context3: load_sym(codec_lib, c"avcodec_alloc_context3")?,
            avcodec_free_context: load_sym(codec_lib, c"avcodec_free_context")?,
            avcodec_open2: load_sym(codec_lib, c"avcodec_open2")?,
            avcodec_send_packet: load_sym(codec_lib, c"avcodec_send_packet")?,
            avcodec_receive_frame: load_sym(codec_lib, c"avcodec_receive_frame")?,
            avcodec_flush_buffers: load_sym(codec_lib, c"avcodec_flush_buffers")?,
            avcodec_default_get_format: load_sym(codec_lib, c"avcodec_default_get_format")?,
            av_packet_alloc: load_sym(codec_lib, c"av_packet_alloc")?,
            av_packet_free: load_sym(codec_lib, c"av_packet_free")?,
            av_new_packet: load_sym(codec_lib, c"av_new_packet")?,
            av_packet_unref: load_sym(codec_lib, c"av_packet_unref")?,
        })
    }
}

fn load_av_symbols(codec_lib: *mut c_void, util_lib: *mut c_void) -> Option<AvApi> {
    // SAFETY: resolves standard FFmpeg C symbols and probes struct offsets on temporary objects.
    unsafe {
        let u = load_avutil_syms(util_lib)?;
        let c = load_avcodec_syms(codec_lib)?;
        let offsets = probe_offsets(
            c.avcodec_alloc_context3,
            c.avcodec_free_context,
            c.avcodec_default_get_format,
            u.av_opt_find,
            u.av_frame_alloc,
            u.av_frame_free,
        )?;
        let pix_fmts = AvPixelFormats {
            vaapi: (u.av_get_pix_fmt)(c"vaapi".as_ptr()),
            nv12: (u.av_get_pix_fmt)(c"nv12".as_ptr()),
            p010le: (u.av_get_pix_fmt)(c"p010le".as_ptr()),
            yuv420p: (u.av_get_pix_fmt)(c"yuv420p".as_ptr()),
            yuvj420p: (u.av_get_pix_fmt)(c"yuvj420p".as_ptr()),
            yuv420p10le: (u.av_get_pix_fmt)(c"yuv420p10le".as_ptr()),
        };
        VAAPI_PIX_FMT_GLOBAL.store(pix_fmts.vaapi, Ordering::Relaxed);
        let vaapi_device_type = (u.av_hwdevice_find_type_by_name)(c"vaapi".as_ptr());

        Some(AvApi {
            _libavutil: util_lib,
            _libavcodec: codec_lib,
            av_frame_alloc: u.av_frame_alloc,
            av_frame_free: u.av_frame_free,
            av_frame_unref: u.av_frame_unref,
            av_frame_clone: u.av_frame_clone,
            av_buffer_ref: u.av_buffer_ref,
            av_buffer_unref: u.av_buffer_unref,
            av_hwdevice_ctx_create: u.av_hwdevice_ctx_create,
            av_opt_set_int: u.av_opt_set_int,
            avcodec_find_decoder_by_name: c.avcodec_find_decoder_by_name,
            avcodec_alloc_context3: c.avcodec_alloc_context3,
            avcodec_free_context: c.avcodec_free_context,
            avcodec_open2: c.avcodec_open2,
            avcodec_send_packet: c.avcodec_send_packet,
            avcodec_receive_frame: c.avcodec_receive_frame,
            avcodec_flush_buffers: c.avcodec_flush_buffers,
            avcodec_default_get_format: c.avcodec_default_get_format,
            av_packet_alloc: c.av_packet_alloc,
            av_packet_free: c.av_packet_free,
            av_new_packet: c.av_new_packet,
            av_packet_unref: c.av_packet_unref,
            vaapi_device_type,
            pix_fmts,
            offsets,
        })
    }
}

unsafe fn probe_offsets(
    alloc_ctx: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    free_ctx: unsafe extern "C" fn(*mut *mut c_void),
    default_get_format: unsafe extern "C" fn(*mut c_void, *const c_int) -> c_int,
    opt_find: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        c_int,
        c_int,
    ) -> *const AvOption,
    alloc_frame: unsafe extern "C" fn() -> *mut c_void,
    free_frame: unsafe extern "C" fn(*mut *mut c_void),
) -> Option<AvOffsets> {
    // SAFETY: allocates a temporary `AVCodecContext` and `AVFrame` to locate exact field offsets.
    unsafe {
        let mut ctx = alloc_ctx(ptr::null());
        if ctx.is_null() {
            return None;
        }
        let target_fn = default_get_format as usize;
        let words = std::slice::from_raw_parts(ctx.cast::<usize>(), 128);
        let get_format = words
            .iter()
            .position(|&word| word == target_fn)
            .map(|idx| idx * size_of::<usize>());
        let opt = opt_find(ctx, c"hwaccel_flags".as_ptr(), ptr::null(), 0, 0);
        let hw_device_ctx = (!opt.is_null())
            .then(|| usize::try_from((*opt).offset).ok())
            .flatten()
            .and_then(|off| off.checked_sub(size_of::<*mut c_void>()));
        free_ctx(&mut ctx);

        let mut frame = alloc_frame();
        if frame.is_null() {
            return None;
        }
        let qwords = std::slice::from_raw_parts(frame.cast::<i64>(), 40);
        let frame_pts = (15..qwords.len())
            .find(|&idx| qwords[idx] == AV_NOPTS_VALUE)
            .map(|idx| idx * size_of::<i64>());
        free_frame(&mut frame);

        Some(AvOffsets {
            get_format: get_format?,
            hw_device_ctx: hw_device_ctx?,
            frame_pts: frame_pts?,
        })
    }
}
