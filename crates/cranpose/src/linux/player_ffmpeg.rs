//! Runtime `dlopen` loader and ABI layout prober for Linux `libavformat`, `libavcodec`, and
//! `libavutil`.
//!
//! Avoids compile-time C headers and probes `AVFormatContext` / `AVStream` offsets at startup so a
//! single binary runs cleanly across FFmpeg 5, 6, 7, 8, and 9 distributions.
#![expect(
    unsafe_code,
    reason = "dlopen/dlsym and C ABI function pointers for Linux FFmpeg shared libraries"
)]

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
    sync::OnceLock,
};

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, VideoCodec, VideoColorInfo, VideoRotation,
};

pub(super) const AV_NOPTS_VALUE: i64 = i64::MIN;
pub(super) const AV_TIME_BASE: f64 = 1_000_000.0;
pub(super) const AVMEDIA_TYPE_VIDEO: c_int = 0;
pub(super) const AVMEDIA_TYPE_AUDIO: c_int = 1;
pub(super) const AV_PKT_FLAG_KEY: c_int = 1;
pub(super) const AVSEEK_FLAG_BACKWARD: c_int = 1;
pub(super) const AVSEEK_SIZE: c_int = 0x1_0000;
pub(super) const AVERROR_EOF: c_int = -(i32::from_le_bytes(*b"EOF "));
pub(super) const AVERROR_EAGAIN: c_int = -libc::EAGAIN;
pub(super) const AV_OPT_SEARCH_CHILDREN: c_int = 1;
pub(super) const AVIO_BUF_SIZE: usize = 64 * 1024;

pub(super) const AV_SAMPLE_FMT_U8: c_int = 0;
pub(super) const AV_SAMPLE_FMT_S16: c_int = 1;
pub(super) const AV_SAMPLE_FMT_S32: c_int = 2;
pub(super) const AV_SAMPLE_FMT_FLT: c_int = 3;
pub(super) const AV_SAMPLE_FMT_DBL: c_int = 4;
pub(super) const AV_SAMPLE_FMT_U8P: c_int = 5;
pub(super) const AV_SAMPLE_FMT_S16P: c_int = 6;
pub(super) const AV_SAMPLE_FMT_S32P: c_int = 7;
pub(super) const AV_SAMPLE_FMT_FLTP: c_int = 8;
pub(super) const AV_SAMPLE_FMT_DBLP: c_int = 9;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct AvRational {
    pub(super) num: c_int,
    pub(super) den: c_int,
}

#[repr(C)]
pub(super) struct AvPacketHead {
    pub(super) buf: *mut c_void,
    pub(super) pts: i64,
    pub(super) dts: i64,
    pub(super) data: *mut u8,
    pub(super) size: c_int,
    pub(super) stream_index: c_int,
    pub(super) flags: c_int,
}

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

#[repr(C)]
pub(super) struct AvDictEntry {
    pub(super) key: *const c_char,
    pub(super) value: *const c_char,
}

pub(super) type AvioReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
pub(super) type AvioSeekFn = unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64;

pub(super) struct AvLayout {
    pub(super) fmt_pb: usize,
    pub(super) fmt_nb_streams: usize,
    pub(super) fmt_streams: usize,
    pub(super) fmt_duration: usize,
    pub(super) stream_codecpar: usize,
    pub(super) stream_time_base: usize,
    pub(super) stream_duration: usize,
    pub(super) stream_metadata: usize,
    pub(super) par_width: usize,
    pub(super) par_height: usize,
}

pub(super) struct FfmpegApi {
    _libavformat: *mut c_void,
    _libavcodec: *mut c_void,
    _libavutil: *mut c_void,
    pub(super) layout: AvLayout,
    pub(super) avformat_alloc_context: unsafe extern "C" fn() -> *mut c_void,
    pub(super) avformat_free_context: unsafe extern "C" fn(*mut c_void),
    pub(super) avformat_open_input: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> c_int,
    pub(super) avformat_close_input: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) avformat_find_stream_info:
        unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    pub(super) av_read_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub(super) av_seek_frame: unsafe extern "C" fn(*mut c_void, c_int, i64, c_int) -> c_int,
    pub(super) avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<AvioReadFn>,
        Option<AvioReadFn>,
        Option<AvioSeekFn>,
    ) -> *mut c_void,
    pub(super) avio_context_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) avcodec_get_name: unsafe extern "C" fn(u32) -> *const c_char,
    pub(super) avcodec_find_decoder: unsafe extern "C" fn(u32) -> *const c_void,
    pub(super) avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    pub(super) avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) avcodec_parameters_to_context:
        unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub(super) avcodec_parameters_copy: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub(super) avcodec_open2:
        unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    pub(super) avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    pub(super) avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub(super) avcodec_flush_buffers: unsafe extern "C" fn(*mut c_void),
    pub(super) av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub(super) av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_packet_unref: unsafe extern "C" fn(*mut c_void),
    pub(super) av_bsf_get_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    pub(super) av_bsf_alloc: unsafe extern "C" fn(*const c_void, *mut *mut c_void) -> c_int,
    pub(super) av_bsf_init: unsafe extern "C" fn(*mut c_void) -> c_int,
    pub(super) av_bsf_send_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub(super) av_bsf_receive_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    pub(super) av_bsf_flush: unsafe extern "C" fn(*mut c_void),
    pub(super) av_bsf_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    pub(super) av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    pub(super) av_frame_unref: unsafe extern "C" fn(*mut c_void),
    pub(super) av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    pub(super) av_free: unsafe extern "C" fn(*mut c_void),
    pub(super) av_dict_get: unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const AvDictEntry,
        c_int,
    ) -> *const AvDictEntry,
    pub(super) av_opt_get_int:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut i64) -> c_int,
    pub(super) av_opt_get:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut u8) -> c_int,
}

// SAFETY: process-wide immutable dlopen library handles and C function pointers.
unsafe impl Send for FfmpegApi {}
// SAFETY: as for Send.
unsafe impl Sync for FfmpegApi {}

fn dlopen_any(names: &[&CStr]) -> *mut c_void {
    for name in names {
        // SAFETY: `name` is NUL-terminated.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
        if !handle.is_null() {
            return handle;
        }
    }
    ptr::null_mut()
}

unsafe fn load_sym<T>(lib: *mut c_void, name: &CStr) -> Option<T> {
    let raw = unsafe { libc::dlsym(lib, name.as_ptr()) };
    if raw.is_null() {
        return None;
    }
    // SAFETY: caller matches the exact C signature of `name` in FFmpeg's public headers.
    Some(unsafe { std::mem::transmute_copy(&raw) })
}

pub(super) fn ffmpeg_api() -> Option<&'static FfmpegApi> {
    static API: OnceLock<Option<FfmpegApi>> = OnceLock::new();
    API.get_or_init(load_ffmpeg_api).as_ref()
}

pub(crate) fn avformat_available() -> bool {
    ffmpeg_api().is_some()
}

fn load_ffmpeg_api() -> Option<FfmpegApi> {
    let libavutil = dlopen_any(&[
        c"libavutil.so.61",
        c"libavutil.so.60",
        c"libavutil.so.59",
        c"libavutil.so.58",
        c"libavutil.so.57",
        c"libavutil.so.56",
        c"libavutil.so",
    ]);
    let libavcodec = dlopen_any(&[
        c"libavcodec.so.63",
        c"libavcodec.so.62",
        c"libavcodec.so.61",
        c"libavcodec.so.60",
        c"libavcodec.so.59",
        c"libavcodec.so.58",
        c"libavcodec.so",
    ]);
    let libavformat = dlopen_any(&[
        c"libavformat.so.63",
        c"libavformat.so.62",
        c"libavformat.so.61",
        c"libavformat.so.60",
        c"libavformat.so.59",
        c"libavformat.so.58",
        c"libavformat.so",
    ]);
    if libavutil.is_null() || libavcodec.is_null() || libavformat.is_null() {
        return None;
    }

    // SAFETY: resolves documented public FFmpeg C API symbols.
    unsafe {
        let fmt = load_format_syms(libavformat)?;
        let codec = load_codec_syms(libavcodec)?;
        let bsf = load_bsf_syms(libavcodec)?;
        let util = load_util_syms(libavutil)?;
        let layout = probe_layout(
            (fmt.avformat_version)() >> 16,
            fmt.avformat_alloc_context,
            fmt.avformat_free_context,
            fmt.avformat_new_stream,
        )?;
        Some(FfmpegApi {
            _libavformat: libavformat,
            _libavcodec: libavcodec,
            _libavutil: libavutil,
            layout,
            avformat_alloc_context: fmt.avformat_alloc_context,
            avformat_free_context: fmt.avformat_free_context,
            avformat_open_input: fmt.avformat_open_input,
            avformat_close_input: fmt.avformat_close_input,
            avformat_find_stream_info: fmt.avformat_find_stream_info,
            av_read_frame: fmt.av_read_frame,
            av_seek_frame: fmt.av_seek_frame,
            avio_alloc_context: fmt.avio_alloc_context,
            avio_context_free: fmt.avio_context_free,
            avcodec_get_name: codec.avcodec_get_name,
            avcodec_find_decoder: codec.avcodec_find_decoder,
            avcodec_alloc_context3: codec.avcodec_alloc_context3,
            avcodec_free_context: codec.avcodec_free_context,
            avcodec_parameters_to_context: codec.avcodec_parameters_to_context,
            avcodec_parameters_copy: codec.avcodec_parameters_copy,
            avcodec_open2: codec.avcodec_open2,
            avcodec_send_packet: codec.avcodec_send_packet,
            avcodec_receive_frame: codec.avcodec_receive_frame,
            avcodec_flush_buffers: codec.avcodec_flush_buffers,
            av_packet_alloc: codec.av_packet_alloc,
            av_packet_free: codec.av_packet_free,
            av_packet_unref: codec.av_packet_unref,
            av_bsf_get_by_name: bsf.av_bsf_get_by_name,
            av_bsf_alloc: bsf.av_bsf_alloc,
            av_bsf_init: bsf.av_bsf_init,
            av_bsf_send_packet: bsf.av_bsf_send_packet,
            av_bsf_receive_packet: bsf.av_bsf_receive_packet,
            av_bsf_flush: bsf.av_bsf_flush,
            av_bsf_free: bsf.av_bsf_free,
            av_frame_alloc: util.av_frame_alloc,
            av_frame_free: util.av_frame_free,
            av_frame_unref: util.av_frame_unref,
            av_malloc: util.av_malloc,
            av_free: util.av_free,
            av_dict_get: util.av_dict_get,
            av_opt_get_int: util.av_opt_get_int,
            av_opt_get: util.av_opt_get,
        })
    }
}

struct FormatSyms {
    avformat_version: unsafe extern "C" fn() -> u32,
    avformat_alloc_context: unsafe extern "C" fn() -> *mut c_void,
    avformat_free_context: unsafe extern "C" fn(*mut c_void),
    avformat_new_stream: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void,
    avformat_open_input: unsafe extern "C" fn(
        *mut *mut c_void,
        *const c_char,
        *const c_void,
        *mut *mut c_void,
    ) -> c_int,
    avformat_close_input: unsafe extern "C" fn(*mut *mut c_void),
    avformat_find_stream_info: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> c_int,
    av_read_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_seek_frame: unsafe extern "C" fn(*mut c_void, c_int, i64, c_int) -> c_int,
    avio_alloc_context: unsafe extern "C" fn(
        *mut u8,
        c_int,
        c_int,
        *mut c_void,
        Option<AvioReadFn>,
        Option<AvioReadFn>,
        Option<AvioSeekFn>,
    ) -> *mut c_void,
    avio_context_free: unsafe extern "C" fn(*mut *mut c_void),
}

struct CodecSyms {
    avcodec_get_name: unsafe extern "C" fn(u32) -> *const c_char,
    avcodec_find_decoder: unsafe extern "C" fn(u32) -> *const c_void,
    avcodec_alloc_context3: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    avcodec_free_context: unsafe extern "C" fn(*mut *mut c_void),
    avcodec_parameters_to_context: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_parameters_copy: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_open2: unsafe extern "C" fn(*mut c_void, *const c_void, *mut *mut c_void) -> c_int,
    avcodec_send_packet: unsafe extern "C" fn(*mut c_void, *const c_void) -> c_int,
    avcodec_receive_frame: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    avcodec_flush_buffers: unsafe extern "C" fn(*mut c_void),
    av_packet_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_packet_free: unsafe extern "C" fn(*mut *mut c_void),
    av_packet_unref: unsafe extern "C" fn(*mut c_void),
}

struct BsfSyms {
    av_bsf_get_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    av_bsf_alloc: unsafe extern "C" fn(*const c_void, *mut *mut c_void) -> c_int,
    av_bsf_init: unsafe extern "C" fn(*mut c_void) -> c_int,
    av_bsf_send_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_bsf_receive_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_bsf_flush: unsafe extern "C" fn(*mut c_void),
    av_bsf_free: unsafe extern "C" fn(*mut *mut c_void),
}

struct UtilSyms {
    av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    av_frame_unref: unsafe extern "C" fn(*mut c_void),
    av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    av_free: unsafe extern "C" fn(*mut c_void),
    av_dict_get: unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const AvDictEntry,
        c_int,
    ) -> *const AvDictEntry,
    av_opt_get_int: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut i64) -> c_int,
    av_opt_get: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut u8) -> c_int,
}

unsafe fn load_format_syms(lib: *mut c_void) -> Option<FormatSyms> {
    unsafe {
        Some(FormatSyms {
            avformat_version: load_sym(lib, c"avformat_version")?,
            avformat_alloc_context: load_sym(lib, c"avformat_alloc_context")?,
            avformat_free_context: load_sym(lib, c"avformat_free_context")?,
            avformat_new_stream: load_sym(lib, c"avformat_new_stream")?,
            avformat_open_input: load_sym(lib, c"avformat_open_input")?,
            avformat_close_input: load_sym(lib, c"avformat_close_input")?,
            avformat_find_stream_info: load_sym(lib, c"avformat_find_stream_info")?,
            av_read_frame: load_sym(lib, c"av_read_frame")?,
            av_seek_frame: load_sym(lib, c"av_seek_frame")?,
            avio_alloc_context: load_sym(lib, c"avio_alloc_context")?,
            avio_context_free: load_sym(lib, c"avio_context_free")?,
        })
    }
}

unsafe fn load_codec_syms(lib: *mut c_void) -> Option<CodecSyms> {
    unsafe {
        Some(CodecSyms {
            avcodec_get_name: load_sym(lib, c"avcodec_get_name")?,
            avcodec_find_decoder: load_sym(lib, c"avcodec_find_decoder")?,
            avcodec_alloc_context3: load_sym(lib, c"avcodec_alloc_context3")?,
            avcodec_free_context: load_sym(lib, c"avcodec_free_context")?,
            avcodec_parameters_to_context: load_sym(lib, c"avcodec_parameters_to_context")?,
            avcodec_parameters_copy: load_sym(lib, c"avcodec_parameters_copy")?,
            avcodec_open2: load_sym(lib, c"avcodec_open2")?,
            avcodec_send_packet: load_sym(lib, c"avcodec_send_packet")?,
            avcodec_receive_frame: load_sym(lib, c"avcodec_receive_frame")?,
            avcodec_flush_buffers: load_sym(lib, c"avcodec_flush_buffers")?,
            av_packet_alloc: load_sym(lib, c"av_packet_alloc")?,
            av_packet_free: load_sym(lib, c"av_packet_free")?,
            av_packet_unref: load_sym(lib, c"av_packet_unref")?,
        })
    }
}

unsafe fn load_bsf_syms(lib: *mut c_void) -> Option<BsfSyms> {
    unsafe {
        Some(BsfSyms {
            av_bsf_get_by_name: load_sym(lib, c"av_bsf_get_by_name")?,
            av_bsf_alloc: load_sym(lib, c"av_bsf_alloc")?,
            av_bsf_init: load_sym(lib, c"av_bsf_init")?,
            av_bsf_send_packet: load_sym(lib, c"av_bsf_send_packet")?,
            av_bsf_receive_packet: load_sym(lib, c"av_bsf_receive_packet")?,
            av_bsf_flush: load_sym(lib, c"av_bsf_flush")?,
            av_bsf_free: load_sym(lib, c"av_bsf_free")?,
        })
    }
}

unsafe fn load_util_syms(lib: *mut c_void) -> Option<UtilSyms> {
    unsafe {
        Some(UtilSyms {
            av_frame_alloc: load_sym(lib, c"av_frame_alloc")?,
            av_frame_free: load_sym(lib, c"av_frame_free")?,
            av_frame_unref: load_sym(lib, c"av_frame_unref")?,
            av_malloc: load_sym(lib, c"av_malloc")?,
            av_free: load_sym(lib, c"av_free")?,
            av_dict_get: load_sym(lib, c"av_dict_get")?,
            av_opt_get_int: load_sym(lib, c"av_opt_get_int")?,
            av_opt_get: load_sym(lib, c"av_opt_get")?,
        })
    }
}

unsafe fn probe_layout(
    major: u32,
    alloc_ctx: unsafe extern "C" fn() -> *mut c_void,
    free_ctx: unsafe extern "C" fn(*mut c_void),
    new_stream: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void,
) -> Option<AvLayout> {
    let ctx = unsafe { alloc_ctx() };
    if ctx.is_null() {
        return None;
    }
    let stream = unsafe { new_stream(ctx, ptr::null()) };
    if stream.is_null() {
        unsafe { free_ctx(ctx) };
        return None;
    }
    let ptr_sz = size_of::<*const c_void>();
    let fmt_pb = 4 * ptr_sz;
    let (fmt_nb_streams, fmt_streams) = unsafe { find_streams_offsets(ctx, stream) }?;
    let fmt_duration = unsafe { find_nopts_pair_second(ctx, 256) }.unwrap_or(104);
    let (stream_codecpar, stream_time_base, stream_duration, stream_metadata) = if major >= 61 {
        (2 * ptr_sz, 4 * ptr_sz, 6 * ptr_sz, 10 * ptr_sz)
    } else {
        let codecpar = unsafe { find_codecpar_offset(stream) }?;
        (codecpar, 2 * ptr_sz, 4 * ptr_sz, 8 * ptr_sz)
    };
    let par_ptr = unsafe {
        ptr::read_unaligned(
            stream
                .cast::<u8>()
                .add(stream_codecpar)
                .cast::<*const c_void>(),
        )
    };
    let (par_width, par_height) = unsafe { find_par_dimensions_offset(par_ptr) }?;
    unsafe { free_ctx(ctx) };
    Some(AvLayout {
        fmt_pb,
        fmt_nb_streams,
        fmt_streams,
        fmt_duration,
        stream_codecpar,
        stream_time_base,
        stream_duration,
        stream_metadata,
        par_width,
        par_height,
    })
}

unsafe fn find_par_dimensions_offset(par: *const c_void) -> Option<(usize, usize)> {
    if par.is_null() {
        return None;
    }
    let base = par.cast::<u8>();
    // `avcodec_parameters_alloc` sets `profile = -99` (`AV_PROFILE_UNKNOWN`) and
    // `level = -99` (`AV_LEVEL_UNKNOWN`) immediately before `int width; int height;`.
    let mut off = 24;
    while off + 16 <= 128 {
        let a = unsafe { ptr::read_unaligned(base.add(off).cast::<i32>()) };
        let b = unsafe { ptr::read_unaligned(base.add(off + 4).cast::<i32>()) };
        let w = unsafe { ptr::read_unaligned(base.add(off + 8).cast::<i32>()) };
        let h = unsafe { ptr::read_unaligned(base.add(off + 12).cast::<i32>()) };
        if a == -99 && b == -99 && w == 0 && h == 0 {
            return Some((off + 8, off + 12));
        }
        off += 4;
    }
    None
}

unsafe fn find_streams_offsets(ctx: *mut c_void, stream: *mut c_void) -> Option<(usize, usize)> {
    let base = ctx.cast::<u8>();
    let ptr_sz = size_of::<*const c_void>();
    let mut off = 4 * ptr_sz;
    while off + 16 <= 128 {
        let nb = unsafe { ptr::read_unaligned(base.add(off).cast::<u32>()) };
        let streams_off = (off + 4).next_multiple_of(ptr_sz);
        let streams_ptr =
            unsafe { ptr::read_unaligned(base.add(streams_off).cast::<*const *mut c_void>()) };
        if nb == 1 && !streams_ptr.is_null() {
            let first = unsafe { *streams_ptr };
            if first == stream {
                return Some((off, streams_off));
            }
        }
        off += 4;
    }
    None
}

unsafe fn find_nopts_pair_second(ctx: *mut c_void, limit: usize) -> Option<usize> {
    let base = ctx.cast::<u8>();
    let mut off = 64;
    while off + 16 <= limit {
        let a = unsafe { ptr::read_unaligned(base.add(off).cast::<i64>()) };
        let b = unsafe { ptr::read_unaligned(base.add(off + 8).cast::<i64>()) };
        if a == AV_NOPTS_VALUE && b == AV_NOPTS_VALUE {
            return Some(off + 8);
        }
        off += 8;
    }
    None
}

unsafe fn find_codecpar_offset(stream: *mut c_void) -> Option<usize> {
    let base = stream.cast::<u8>();
    let ptr_sz = size_of::<*const c_void>();
    let mut off = 12 * ptr_sz;
    while off + ptr_sz <= 320 {
        let candidate = unsafe { ptr::read_unaligned(base.add(off).cast::<*const c_int>()) };
        if !candidate.is_null() && (candidate as usize).is_multiple_of(4) {
            let codec_type = unsafe { *candidate };
            let codec_id = unsafe { *candidate.add(1) };
            if codec_type == -1 && codec_id == 0 {
                return Some(off);
            }
        }
        off += ptr_sz;
    }
    None
}

#[derive(Default)]
pub(super) struct CodecProps {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bitrate: Option<u32>,
    pub(super) color: Option<VideoColorInfo>,
    pub(super) sample_rate: Option<u32>,
    pub(super) channels: Option<usize>,
}

pub(super) fn probe_codec_props(api: &'static FfmpegApi, par: *const c_void) -> CodecProps {
    let raw_w =
        unsafe { ptr::read_unaligned(par.cast::<u8>().add(api.layout.par_width).cast::<c_int>()) };
    let raw_h =
        unsafe { ptr::read_unaligned(par.cast::<u8>().add(api.layout.par_height).cast::<c_int>()) };
    let width = u32::try_from(raw_w).unwrap_or(0);
    let height = u32::try_from(raw_h).unwrap_or(0);
    let mut ctx = unsafe { (api.avcodec_alloc_context3)(ptr::null()) };
    if ctx.is_null() {
        return CodecProps {
            width,
            height,
            ..CodecProps::default()
        };
    }
    let ok = unsafe { (api.avcodec_parameters_to_context)(ctx, par) } >= 0;
    let props = if ok {
        CodecProps {
            width,
            height,
            bitrate: read_opt_u32(api, ctx, c"b"),
            color: read_opt_color(api, ctx),
            sample_rate: read_opt_u32(api, ctx, c"ar"),
            channels: read_opt_channels(api, ctx),
        }
    } else {
        CodecProps {
            width,
            height,
            ..CodecProps::default()
        }
    };
    unsafe { (api.avcodec_free_context)(&mut ctx) };
    props
}

pub(super) fn read_opt_i64(api: &'static FfmpegApi, obj: *mut c_void, name: &CStr) -> Option<i64> {
    let mut val: i64 = 0;
    let rc = unsafe { (api.av_opt_get_int)(obj, name.as_ptr(), AV_OPT_SEARCH_CHILDREN, &mut val) };
    (rc >= 0).then_some(val)
}

pub(super) fn read_opt_u32(api: &'static FfmpegApi, obj: *mut c_void, name: &CStr) -> Option<u32> {
    read_opt_i64(api, obj, name)
        .filter(|&v| v > 0)
        .and_then(|v| u32::try_from(v).ok())
}

pub(super) fn read_opt_channels(api: &'static FfmpegApi, ctx: *mut c_void) -> Option<usize> {
    if let Some(ch) = read_opt_u32(api, ctx, c"ac") {
        return usize::try_from(ch).ok();
    }
    let mut out: *mut u8 = ptr::null_mut();
    let rc =
        unsafe { (api.av_opt_get)(ctx, c"ch_layout".as_ptr(), AV_OPT_SEARCH_CHILDREN, &mut out) };
    if rc < 0 || out.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(out.cast::<c_char>()) }
        .to_string_lossy()
        .into_owned();
    unsafe { (api.av_free)(out.cast()) };
    parse_channel_layout_count(&text)
}

fn parse_channel_layout_count(text: &str) -> Option<usize> {
    match text.trim() {
        "mono" => Some(1),
        "stereo" | "downmix" => Some(2),
        "2.1" | "3.0" => Some(3),
        "quad" | "4.0" => Some(4),
        "5.0" | "5.0(side)" => Some(5),
        "5.1" | "5.1(side)" => Some(6),
        "6.1" => Some(7),
        "7.1" => Some(8),
        other => other
            .split_whitespace()
            .next()
            .and_then(|tok| tok.parse::<usize>().ok())
            .filter(|&n| n > 0),
    }
}

fn read_opt_color(api: &'static FfmpegApi, ctx: *mut c_void) -> Option<VideoColorInfo> {
    let primaries = match read_opt_i64(api, ctx, c"color_primaries") {
        Some(1) => Some(ColorPrimaries::Bt709),
        Some(5 | 6) => Some(ColorPrimaries::Bt601),
        Some(9) => Some(ColorPrimaries::Bt2020),
        _ => None,
    };
    let range = match read_opt_i64(api, ctx, c"color_range") {
        Some(1) => Some(ColorRange::Limited),
        Some(2) => Some(ColorRange::Full),
        _ => None,
    };
    let transfer = match read_opt_i64(api, ctx, c"color_trc") {
        Some(1 | 6 | 13) => Some(ColorTransfer::Sdr),
        Some(16) => Some(ColorTransfer::Pq),
        Some(18) => Some(ColorTransfer::Hlg),
        _ => None,
    };
    crate::player_demux_engine::assemble_color_info(primaries, range, transfer)
}

pub(super) fn codec_name_of(api: &'static FfmpegApi, codec_id: u32) -> String {
    let ptr = unsafe { (api.avcodec_get_name)(codec_id) };
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

pub(super) fn video_codec_from_name(name: &str) -> Option<VideoCodec> {
    match name {
        "h264" => Some(VideoCodec::H264),
        "hevc" => Some(VideoCodec::Hevc),
        "vp9" => Some(VideoCodec::Vp9),
        "av1" => Some(VideoCodec::Av1),
        _ => None,
    }
}

pub(super) fn read_stream_metadata(
    api: &'static FfmpegApi,
    stream: *mut c_void,
    key: &CStr,
) -> Option<String> {
    let dict = unsafe {
        ptr::read_unaligned(
            stream
                .cast::<u8>()
                .add(api.layout.stream_metadata)
                .cast::<*const c_void>(),
        )
    };
    if dict.is_null() {
        return None;
    }
    let entry = unsafe { (api.av_dict_get)(dict, key.as_ptr(), ptr::null(), 0) };
    if entry.is_null() {
        return None;
    }
    let val = unsafe { (*entry).value };
    if val.is_null() {
        return None;
    }
    Some(
        unsafe { CStr::from_ptr(val) }
            .to_string_lossy()
            .into_owned(),
    )
}

pub(super) fn rotation_from_degrees(degrees: i32) -> VideoRotation {
    match degrees.rem_euclid(360) {
        90 => VideoRotation::Degrees90,
        180 => VideoRotation::Degrees180,
        270 => VideoRotation::Degrees270,
        _ => VideoRotation::Degrees0,
    }
}
