//! Runtime-`dlopen` FFmpeg (`libavformat` + `libavcodec` + `libavutil`) container demuxer,
//! Annex-B bitstream filter, and PCM audio decoder for Linux [`Player`](crate::Player).
//!
//! Avoids compile-time C headers and probes `AVFormatContext` / `AVStream` / `AVCodecContext`
//! offsets at startup so a single binary runs cleanly across FFmpeg 5, 6, 7, 8, and 9 distros.
#![expect(
    unsafe_code,
    reason = "dlopen/dlsym and C ABI FFI for Linux libavformat, libavcodec, and libavutil"
)]

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io::{Read, Seek, SeekFrom},
    path::Path,
    ptr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, TrackEntry, TrackFormat, TrackGroup, TrackType,
    Tracks, VideoCodec, VideoColorInfo, VideoRotation, VideoSize,
};

use crate::player_stream::HttpRangeReader;

const AV_NOPTS_VALUE: i64 = i64::MIN;
const AV_TIME_BASE: f64 = 1_000_000.0;
const AVMEDIA_TYPE_VIDEO: c_int = 0;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AV_PKT_FLAG_KEY: c_int = 1;
const AVSEEK_FLAG_BACKWARD: c_int = 1;
const AVSEEK_SIZE: c_int = 0x1_0000;
const AVERROR_EOF: c_int = -(i32::from_le_bytes(*b"EOF "));
const AVERROR_EAGAIN: c_int = -libc::EAGAIN;
const AV_OPT_SEARCH_CHILDREN: c_int = 1;
const AVIO_BUF_SIZE: usize = 64 * 1024;

const AV_SAMPLE_FMT_U8: c_int = 0;
const AV_SAMPLE_FMT_S16: c_int = 1;
const AV_SAMPLE_FMT_S32: c_int = 2;
const AV_SAMPLE_FMT_FLT: c_int = 3;
const AV_SAMPLE_FMT_DBL: c_int = 4;
const AV_SAMPLE_FMT_U8P: c_int = 5;
const AV_SAMPLE_FMT_S16P: c_int = 6;
const AV_SAMPLE_FMT_S32P: c_int = 7;
const AV_SAMPLE_FMT_FLTP: c_int = 8;
const AV_SAMPLE_FMT_DBLP: c_int = 9;

#[repr(C)]
#[derive(Clone, Copy)]
struct AvRational {
    num: c_int,
    den: c_int,
}

#[repr(C)]
struct AvPacketHead {
    buf: *mut c_void,
    pts: i64,
    dts: i64,
    data: *mut u8,
    size: c_int,
    stream_index: c_int,
    flags: c_int,
}

#[repr(C)]
struct AvFrameHead {
    data: [*mut u8; 8],
    linesize: [c_int; 8],
    extended_data: *mut *mut u8,
    width: c_int,
    height: c_int,
    nb_samples: c_int,
    format: c_int,
}

#[repr(C)]
struct AvDictEntry {
    key: *const c_char,
    value: *const c_char,
}

type AvioReadFn = unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int;
type AvioSeekFn = unsafe extern "C" fn(*mut c_void, i64, c_int) -> i64;

struct AvLayout {
    fmt_pb: usize,
    fmt_nb_streams: usize,
    fmt_streams: usize,
    fmt_duration: usize,
    stream_codecpar: usize,
    stream_time_base: usize,
    stream_duration: usize,
    stream_metadata: usize,
}

struct FfmpegApi {
    _libavformat: *mut c_void,
    _libavcodec: *mut c_void,
    _libavutil: *mut c_void,
    layout: AvLayout,
    avformat_alloc_context: unsafe extern "C" fn() -> *mut c_void,
    avformat_free_context: unsafe extern "C" fn(*mut c_void),
    avformat_open_input:
        unsafe extern "C" fn(*mut *mut c_void, *const c_char, *const c_void, *mut *mut c_void) -> c_int,
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
    av_bsf_get_by_name: unsafe extern "C" fn(*const c_char) -> *const c_void,
    av_bsf_alloc: unsafe extern "C" fn(*const c_void, *mut *mut c_void) -> c_int,
    av_bsf_init: unsafe extern "C" fn(*mut c_void) -> c_int,
    av_bsf_send_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_bsf_receive_packet: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    av_bsf_flush: unsafe extern "C" fn(*mut c_void),
    av_bsf_free: unsafe extern "C" fn(*mut *mut c_void),
    av_frame_alloc: unsafe extern "C" fn() -> *mut c_void,
    av_frame_free: unsafe extern "C" fn(*mut *mut c_void),
    av_frame_unref: unsafe extern "C" fn(*mut c_void),
    av_malloc: unsafe extern "C" fn(usize) -> *mut c_void,
    av_free: unsafe extern "C" fn(*mut c_void),
    av_dict_get:
        unsafe extern "C" fn(*const c_void, *const c_char, *const AvDictEntry, c_int) -> *const AvDictEntry,
    av_opt_get_int: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut i64) -> c_int,
    av_opt_get: unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *mut *mut u8) -> c_int,
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

fn ffmpeg_api() -> Option<&'static FfmpegApi> {
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
        let avformat_version: unsafe extern "C" fn() -> u32 =
            load_sym(libavformat, c"avformat_version")?;
        let avformat_alloc_context: unsafe extern "C" fn() -> *mut c_void =
            load_sym(libavformat, c"avformat_alloc_context")?;
        let avformat_free_context: unsafe extern "C" fn(*mut c_void) =
            load_sym(libavformat, c"avformat_free_context")?;
        let avformat_new_stream: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            load_sym(libavformat, c"avformat_new_stream")?;
        let layout = probe_layout(
            avformat_version() >> 16,
            avformat_alloc_context,
            avformat_free_context,
            avformat_new_stream,
        )?;
        Some(FfmpegApi {
            _libavformat: libavformat,
            _libavcodec: libavcodec,
            _libavutil: libavutil,
            layout,
            avformat_alloc_context,
            avformat_free_context,
            avformat_open_input: load_sym(libavformat, c"avformat_open_input")?,
            avformat_close_input: load_sym(libavformat, c"avformat_close_input")?,
            avformat_find_stream_info: load_sym(libavformat, c"avformat_find_stream_info")?,
            av_read_frame: load_sym(libavformat, c"av_read_frame")?,
            av_seek_frame: load_sym(libavformat, c"av_seek_frame")?,
            avio_alloc_context: load_sym(libavformat, c"avio_alloc_context")?,
            avio_context_free: load_sym(libavformat, c"avio_context_free")?,
            avcodec_get_name: load_sym(libavcodec, c"avcodec_get_name")?,
            avcodec_find_decoder: load_sym(libavcodec, c"avcodec_find_decoder")?,
            avcodec_alloc_context3: load_sym(libavcodec, c"avcodec_alloc_context3")?,
            avcodec_free_context: load_sym(libavcodec, c"avcodec_free_context")?,
            avcodec_parameters_to_context: load_sym(libavcodec, c"avcodec_parameters_to_context")?,
            avcodec_parameters_copy: load_sym(libavcodec, c"avcodec_parameters_copy")?,
            avcodec_open2: load_sym(libavcodec, c"avcodec_open2")?,
            avcodec_send_packet: load_sym(libavcodec, c"avcodec_send_packet")?,
            avcodec_receive_frame: load_sym(libavcodec, c"avcodec_receive_frame")?,
            avcodec_flush_buffers: load_sym(libavcodec, c"avcodec_flush_buffers")?,
            av_packet_alloc: load_sym(libavcodec, c"av_packet_alloc")?,
            av_packet_free: load_sym(libavcodec, c"av_packet_free")?,
            av_packet_unref: load_sym(libavcodec, c"av_packet_unref")?,
            av_bsf_get_by_name: load_sym(libavcodec, c"av_bsf_get_by_name")?,
            av_bsf_alloc: load_sym(libavcodec, c"av_bsf_alloc")?,
            av_bsf_init: load_sym(libavcodec, c"av_bsf_init")?,
            av_bsf_send_packet: load_sym(libavcodec, c"av_bsf_send_packet")?,
            av_bsf_receive_packet: load_sym(libavcodec, c"av_bsf_receive_packet")?,
            av_bsf_flush: load_sym(libavcodec, c"av_bsf_flush")?,
            av_bsf_free: load_sym(libavcodec, c"av_bsf_free")?,
            av_frame_alloc: load_sym(libavutil, c"av_frame_alloc")?,
            av_frame_free: load_sym(libavutil, c"av_frame_free")?,
            av_frame_unref: load_sym(libavutil, c"av_frame_unref")?,
            av_malloc: load_sym(libavutil, c"av_malloc")?,
            av_free: load_sym(libavutil, c"av_free")?,
            av_dict_get: load_sym(libavutil, c"av_dict_get")?,
            av_opt_get_int: load_sym(libavutil, c"av_opt_get_int")?,
            av_opt_get: load_sym(libavutil, c"av_opt_get")?,
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
    })
}

unsafe fn find_streams_offsets(ctx: *mut c_void, stream: *mut c_void) -> Option<(usize, usize)> {
    let base = ctx.cast::<u8>();
    let ptr_sz = size_of::<*const c_void>();
    let mut off = 4 * ptr_sz;
    while off + 16 <= 128 {
        let nb = unsafe { ptr::read_unaligned(base.add(off).cast::<u32>()) };
        let streams_off = (off + 4).next_multiple_of(ptr_sz);
        let streams_ptr = unsafe { ptr::read_unaligned(base.add(streams_off).cast::<*const *mut c_void>()) };
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

#[derive(Clone)]
pub(crate) struct VideoTrackInfo {
    pub(crate) stream_index: usize,
    pub(crate) codec: VideoCodec,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rotation: VideoRotation,
    pub(crate) color: Option<VideoColorInfo>,
}

#[derive(Clone)]
pub(crate) struct AudioTrackInfo {
    pub(crate) stream_index: usize,
    pub(crate) language: Option<String>,
}

pub(crate) struct DemuxedTracks {
    pub(crate) video: Option<VideoTrackInfo>,
    pub(crate) audio_tracks: Vec<AudioTrackInfo>,
    pub(crate) duration: Option<Duration>,
    pub(crate) tracks: Tracks,
}

pub(crate) struct DemuxedPacket {
    pub(crate) stream_index: usize,
    pub(crate) pts: Duration,
    pub(crate) is_keyframe: bool,
    pub(crate) data: Vec<u8>,
}

pub(crate) struct DecodedAudio {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
}

enum CustomIo {
    Memory { bytes: Arc<[u8]>, pos: usize },
    Http(Box<HttpRangeReader>),
}

struct AvioBacking {
    api: &'static FfmpegApi,
    avio: *mut c_void,
    userdata: *mut CustomIo,
}

impl Drop for AvioBacking {
    fn drop(&mut self) {
        // SAFETY: `Demuxer::drop` closes `AVFormatContext` before `AvioBacking` drops.
        unsafe {
            if !self.avio.is_null() {
                let buf = *(self.avio.cast::<*mut c_void>());
                if !buf.is_null() {
                    (self.api.av_free)(buf);
                }
                (self.api.avio_context_free)(&mut self.avio);
            }
            drop(Box::from_raw(self.userdata));
        }
    }
}

unsafe extern "C" fn custom_avio_read(opaque: *mut c_void, buf: *mut u8, buf_size: c_int) -> c_int {
    if opaque.is_null() || buf.is_null() || buf_size <= 0 {
        return AVERROR_EOF;
    }
    let len = usize::try_from(buf_size).unwrap_or(0);
    // SAFETY: `opaque` points to a live `CustomIo` owned by `AvioBacking`.
    let io = unsafe { &mut *opaque.cast::<CustomIo>() };
    let dst = unsafe { std::slice::from_raw_parts_mut(buf, len) };
    let read = match io {
        CustomIo::Memory { bytes, pos } => {
            if *pos >= bytes.len() {
                return AVERROR_EOF;
            }
            let avail = bytes.len() - *pos;
            let count = avail.min(len);
            dst[..count].copy_from_slice(&bytes[*pos..*pos + count]);
            *pos += count;
            count
        }
        CustomIo::Http(reader) => match reader.read(dst) {
            Ok(0) => return AVERROR_EOF,
            Ok(n) => n,
            Err(_) => return AVERROR_EOF,
        },
    };
    c_int::try_from(read).unwrap_or(buf_size)
}

unsafe extern "C" fn custom_avio_seek(opaque: *mut c_void, offset: i64, whence: c_int) -> i64 {
    if opaque.is_null() {
        return -1;
    }
    // SAFETY: `opaque` points to a live `CustomIo` owned by `AvioBacking`.
    let io = unsafe { &mut *opaque.cast::<CustomIo>() };
    if (whence & AVSEEK_SIZE) != 0 {
        return match io {
            CustomIo::Memory { bytes, .. } => i64::try_from(bytes.len()).unwrap_or(-1),
            CustomIo::Http(reader) => reader
                .total_len()
                .and_then(|n| i64::try_from(n).ok())
                .unwrap_or(-1),
        };
    }
    let origin = whence & !AVSEEK_SIZE;
    match io {
        CustomIo::Memory { bytes, pos } => {
            let base = match origin {
                libc::SEEK_SET => 0_i128,
                libc::SEEK_CUR => *pos as i128,
                libc::SEEK_END => bytes.len() as i128,
                _ => return -1,
            };
            let target = base + i128::from(offset);
            if target < 0 || target > bytes.len() as i128 {
                return -1;
            }
            *pos = usize::try_from(target).unwrap_or(bytes.len());
            i64::try_from(*pos).unwrap_or(-1)
        }
        CustomIo::Http(reader) => {
            let seek_from = match origin {
                libc::SEEK_SET => SeekFrom::Start(u64::try_from(offset).unwrap_or(0)),
                libc::SEEK_CUR => SeekFrom::Current(offset),
                libc::SEEK_END => SeekFrom::End(offset),
                _ => return -1,
            };
            reader
                .seek(seek_from)
                .ok()
                .and_then(|pos| i64::try_from(pos).ok())
                .unwrap_or(-1)
        }
    }
}

pub(crate) struct Demuxer {
    api: &'static FfmpegApi,
    fmt_ctx: *mut c_void,
    packet: *mut c_void,
    bsf_packet: *mut c_void,
    video_bsf: *mut c_void,
    video_stream: Option<usize>,
    time_bases: Vec<AvRational>,
    pending: Option<DemuxedPacket>,
    _avio: Option<AvioBacking>,
}

// SAFETY: `Demuxer` is owned and used exclusively by one player worker thread.
unsafe impl Send for Demuxer {}

impl Drop for Demuxer {
    fn drop(&mut self) {
        // SAFETY: closes format context before `_avio` backing drops.
        unsafe {
            if !self.video_bsf.is_null() {
                (self.api.av_bsf_free)(&mut self.video_bsf);
            }
            (self.api.av_packet_free)(&mut self.bsf_packet);
            (self.api.av_packet_free)(&mut self.packet);
            if !self.fmt_ctx.is_null() {
                if self._avio.is_some() {
                    let pb_slot = self
                        .fmt_ctx
                        .cast::<u8>()
                        .add(self.api.layout.fmt_pb)
                        .cast::<*mut c_void>();
                    *pb_slot = ptr::null_mut();
                }
                (self.api.avformat_close_input)(&mut self.fmt_ctx);
            }
        }
    }
}

impl Demuxer {
    pub(crate) fn from_file(path: &Path) -> Result<Self, String> {
        let api = ffmpeg_api().ok_or_else(|| "libavformat is unavailable on this host".to_owned())?;
        let c_path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "media path contains interior NUL byte".to_owned())?;
        let mut fmt_ctx: *mut c_void = ptr::null_mut();
        // SAFETY: `c_path` is NUL-terminated.
        let rc = unsafe {
            (api.avformat_open_input)(&mut fmt_ctx, c_path.as_ptr(), ptr::null(), ptr::null_mut())
        };
        if rc < 0 || fmt_ctx.is_null() {
            return Err(format!("avformat_open_input failed ({rc})"));
        }
        Self::finish_open(api, fmt_ctx, None)
    }

    pub(crate) fn from_bytes(bytes: Arc<[u8]>) -> Result<Self, String> {
        Self::from_custom_io(CustomIo::Memory { bytes, pos: 0 })
    }

    pub(crate) fn from_http(reader: HttpRangeReader) -> Result<Self, String> {
        Self::from_custom_io(CustomIo::Http(Box::new(reader)))
    }

    fn from_custom_io(io: CustomIo) -> Result<Self, String> {
        let api = ffmpeg_api().ok_or_else(|| "libavformat is unavailable on this host".to_owned())?;
        // SAFETY: allocates AVFormatContext and custom AVIOContext with owned buffer and userdata.
        let mut fmt_ctx = unsafe { (api.avformat_alloc_context)() };
        if fmt_ctx.is_null() {
            return Err("avformat_alloc_context returned null".into());
        }
        let buf = unsafe { (api.av_malloc)(AVIO_BUF_SIZE) }.cast::<u8>();
        if buf.is_null() {
            unsafe { (api.avformat_free_context)(fmt_ctx) };
            return Err("av_malloc failed for AVIO buffer".into());
        }
        let userdata = Box::into_raw(Box::new(io));
        let avio = unsafe {
            (api.avio_alloc_context)(
                buf,
                c_int::try_from(AVIO_BUF_SIZE).unwrap_or(65_536),
                0,
                userdata.cast(),
                Some(custom_avio_read),
                None,
                Some(custom_avio_seek),
            )
        };
        if avio.is_null() {
            unsafe {
                (api.av_free)(buf.cast());
                drop(Box::from_raw(userdata));
                (api.avformat_free_context)(fmt_ctx);
            }
            return Err("avio_alloc_context returned null".into());
        }
        let backing = AvioBacking {
            api,
            avio,
            userdata,
        };
        // SAFETY: `fmt_pb` is the verified offset of `AVFormatContext::pb`.
        unsafe {
            *fmt_ctx
                .cast::<u8>()
                .add(api.layout.fmt_pb)
                .cast::<*mut c_void>() = backing.avio;
        }
        let rc = unsafe {
            (api.avformat_open_input)(&mut fmt_ctx, ptr::null(), ptr::null(), ptr::null_mut())
        };
        if rc < 0 || fmt_ctx.is_null() {
            drop(backing);
            return Err(format!("avformat_open_input (custom IO) failed ({rc})"));
        }
        Self::finish_open(api, fmt_ctx, Some(backing))
    }

    fn finish_open(
        api: &'static FfmpegApi,
        mut fmt_ctx: *mut c_void,
        avio: Option<AvioBacking>,
    ) -> Result<Self, String> {
        // SAFETY: probes stream headers and allocates reusable packet handles.
        let info_rc = unsafe { (api.avformat_find_stream_info)(fmt_ctx, ptr::null_mut()) };
        if info_rc < 0 {
            if avio.is_some() {
                unsafe {
                    *fmt_ctx
                        .cast::<u8>()
                        .add(api.layout.fmt_pb)
                        .cast::<*mut c_void>() = ptr::null_mut();
                }
            }
            unsafe { (api.avformat_close_input)(&mut fmt_ctx) };
            drop(avio);
            return Err(format!("avformat_find_stream_info failed ({info_rc})"));
        }
        let packet = unsafe { (api.av_packet_alloc)() };
        let bsf_packet = unsafe { (api.av_packet_alloc)() };
        if packet.is_null() || bsf_packet.is_null() {
            unsafe {
                let mut p = packet;
                let mut bp = bsf_packet;
                (api.av_packet_free)(&mut p);
                (api.av_packet_free)(&mut bp);
                if avio.is_some() {
                    *fmt_ctx
                        .cast::<u8>()
                        .add(api.layout.fmt_pb)
                        .cast::<*mut c_void>() = ptr::null_mut();
                }
                (api.avformat_close_input)(&mut fmt_ctx);
            }
            drop(avio);
            return Err("av_packet_alloc failed".into());
        }
        let mut demuxer = Self {
            api,
            fmt_ctx,
            packet,
            bsf_packet,
            video_bsf: ptr::null_mut(),
            video_stream: None,
            time_bases: Vec::new(),
            pending: None,
            _avio: avio,
        };
        demuxer.cache_time_bases();
        Ok(demuxer)
    }

    fn nb_streams(&self) -> usize {
        // SAFETY: `fmt_nb_streams` is the verified offset of `AVFormatContext::nb_streams`.
        let n = unsafe {
            ptr::read_unaligned(
                self.fmt_ctx
                    .cast::<u8>()
                    .add(self.api.layout.fmt_nb_streams)
                    .cast::<u32>(),
            )
        };
        usize::try_from(n).unwrap_or(0)
    }

    fn stream_ptr(&self, idx: usize) -> Option<*mut c_void> {
        if idx >= self.nb_streams() {
            return None;
        }
        // SAFETY: `fmt_streams` is the verified offset of `AVFormatContext::streams`.
        let arr = unsafe {
            ptr::read_unaligned(
                self.fmt_ctx
                    .cast::<u8>()
                    .add(self.api.layout.fmt_streams)
                    .cast::<*const *mut c_void>(),
            )
        };
        if arr.is_null() {
            return None;
        }
        let stream = unsafe { *arr.add(idx) };
        (!stream.is_null()).then_some(stream)
    }

    fn stream_codecpar(&self, stream: *mut c_void) -> *mut c_void {
        // SAFETY: `stream_codecpar` is the verified offset of `AVStream::codecpar`.
        unsafe {
            ptr::read_unaligned(
                stream
                    .cast::<u8>()
                    .add(self.api.layout.stream_codecpar)
                    .cast::<*mut c_void>(),
            )
        }
    }

    fn cache_time_bases(&mut self) {
        let count = self.nb_streams();
        self.time_bases.clear();
        self.time_bases.reserve(count);
        for idx in 0..count {
            let tb = self.stream_ptr(idx).map_or(
                AvRational { num: 1, den: 1_000 },
                |stream| unsafe {
                    ptr::read_unaligned(
                        stream
                            .cast::<u8>()
                            .add(self.api.layout.stream_time_base)
                            .cast::<AvRational>(),
                    )
                },
            );
            self.time_bases.push(tb);
        }
    }

    pub(crate) fn inspect_tracks(&mut self) -> DemuxedTracks {
        let count = self.nb_streams();
        let mut video: Option<VideoTrackInfo> = None;
        let mut video_entries = Vec::new();
        let mut audio_tracks = Vec::new();
        let mut audio_entries = Vec::new();

        for idx in 0..count {
            let Some(stream) = self.stream_ptr(idx) else {
                continue;
            };
            let par = self.stream_codecpar(stream);
            if par.is_null() {
                continue;
            }
            // SAFETY: `AVCodecParameters` starts with `{ AVMediaType codec_type; AVCodecID codec_id; }`.
            let codec_type = unsafe { *(par.cast::<c_int>()) };
            let codec_id = unsafe { *(par.cast::<u32>().add(1)) };
            let codec_name = codec_name_of(self.api, codec_id);
            let language = read_stream_metadata(self.api, stream, c"language")
                .filter(|l| !l.is_empty() && l != "und");
            let props = probe_codec_props(self.api, par);

            if codec_type == AVMEDIA_TYPE_VIDEO {
                let Some(codec) = video_codec_from_name(&codec_name) else {
                    continue;
                };
                let selected = video.is_none();
                let rotation = read_stream_metadata(self.api, stream, c"rotate")
                    .and_then(|s| s.parse::<i32>().ok())
                    .map_or(VideoRotation::Degrees0, rotation_from_degrees);
                if selected && props.width > 0 && props.height > 0 {
                    video = Some(VideoTrackInfo {
                        stream_index: idx,
                        codec,
                        width: props.width,
                        height: props.height,
                        rotation,
                        color: props.color,
                    });
                    self.setup_video_bsf(idx, codec, par);
                }
                video_entries.push(TrackEntry {
                    format: TrackFormat {
                        id: format!("video-{idx}"),
                        label: None,
                        language,
                        mime_type: Some(codec.mime().to_owned()),
                        codecs: Some(codec_name),
                        bitrate: props.bitrate,
                        width: (props.width > 0).then_some(props.width),
                        height: (props.height > 0).then_some(props.height),
                        frame_rate: None,
                        channel_count: None,
                        sample_rate: None,
                    },
                    supported: true,
                    selected,
                });
            } else if codec_type == AVMEDIA_TYPE_AUDIO {
                let selected = audio_tracks.is_empty();
                audio_tracks.push(AudioTrackInfo {
                    stream_index: idx,
                    language: language.clone(),
                });
                audio_entries.push(TrackEntry {
                    format: TrackFormat {
                        id: format!("audio-{idx}"),
                        label: None,
                        language,
                        mime_type: Some(format!("audio/{codec_name}")),
                        codecs: Some(codec_name),
                        bitrate: props.bitrate,
                        width: None,
                        height: None,
                        frame_rate: None,
                        channel_count: props.channels.and_then(|c| u16::try_from(c).ok()),
                        sample_rate: props.sample_rate,
                    },
                    supported: true,
                    selected,
                });
            }
        }

        let mut groups = Vec::new();
        if !video_entries.is_empty() {
            groups.push(TrackGroup {
                id: "video".into(),
                track_type: TrackType::Video,
                tracks: video_entries,
                adaptive_supported: false,
            });
        }
        if !audio_entries.is_empty() {
            groups.push(TrackGroup {
                id: "audio".into(),
                track_type: TrackType::Audio,
                tracks: audio_entries,
                adaptive_supported: false,
            });
        }
        DemuxedTracks {
            video,
            audio_tracks,
            duration: self.container_duration(),
            tracks: Tracks { groups },
        }
    }

    fn container_duration(&self) -> Option<Duration> {
        // SAFETY: `fmt_duration` is the verified offset of `AVFormatContext::duration`.
        let dur = unsafe {
            ptr::read_unaligned(
                self.fmt_ctx
                    .cast::<u8>()
                    .add(self.api.layout.fmt_duration)
                    .cast::<i64>(),
            )
        };
        if dur > 0 && dur != AV_NOPTS_VALUE {
            return Some(Duration::from_secs_f64(dur as f64 / AV_TIME_BASE));
        }
        for idx in 0..self.nb_streams() {
            let Some(stream) = self.stream_ptr(idx) else {
                continue;
            };
            let sdur = unsafe {
                ptr::read_unaligned(
                    stream
                        .cast::<u8>()
                        .add(self.api.layout.stream_duration)
                        .cast::<i64>(),
                )
            };
            let tb = self.time_bases.get(idx).copied().unwrap_or(AvRational {
                num: 1,
                den: 1_000,
            });
            if sdur > 0 && sdur != AV_NOPTS_VALUE && tb.den > 0 {
                let secs = (sdur as f64) * (f64::from(tb.num) / f64::from(tb.den));
                if secs.is_finite() && secs > 0.0 {
                    return Some(Duration::from_secs_f64(secs));
                }
            }
        }
        None
    }

    fn setup_video_bsf(&mut self, stream_index: usize, codec: VideoCodec, par: *mut c_void) {
        self.video_stream = Some(stream_index);
        if !self.video_bsf.is_null() {
            // SAFETY: frees any prior BSF before creating a new one.
            unsafe { (self.api.av_bsf_free)(&mut self.video_bsf) };
        }
        let bsf_name = match codec {
            VideoCodec::H264 => c"h264_mp4toannexb",
            VideoCodec::Hevc => c"hevc_mp4toannexb",
            VideoCodec::Vp9 | VideoCodec::Av1 => return,
        };
        // SAFETY: initializes FFmpeg bitstream filter for MP4/MKV -> Annex-B conversion.
        unsafe {
            let filter = (self.api.av_bsf_get_by_name)(bsf_name.as_ptr());
            if filter.is_null() {
                return;
            }
            let mut bsf: *mut c_void = ptr::null_mut();
            if (self.api.av_bsf_alloc)(filter, &mut bsf) < 0 || bsf.is_null() {
                return;
            }
            let par_in = *(bsf.cast::<u8>().add(3 * size_of::<*const c_void>()).cast::<*mut c_void>());
            if (self.api.avcodec_parameters_copy)(par_in, par) < 0
                || (self.api.av_bsf_init)(bsf) < 0
            {
                (self.api.av_bsf_free)(&mut bsf);
                return;
            }
            self.video_bsf = bsf;
        }
    }

    pub(crate) fn open_audio_decoder(&self, stream_index: usize) -> Result<AudioDecoder, String> {
        let stream = self
            .stream_ptr(stream_index)
            .ok_or_else(|| format!("audio stream {stream_index} not found"))?;
        let par = self.stream_codecpar(stream);
        if par.is_null() {
            return Err("audio stream missing codecpar".into());
        }
        AudioDecoder::open(self.api, par)
    }

    pub(crate) fn seek_to(&mut self, position: Duration) -> Result<(), String> {
        self.pending = None;
        let ts = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        // SAFETY: seeks default stream timeline backward to nearest keyframe <= `ts`.
        let rc = unsafe { (self.api.av_seek_frame)(self.fmt_ctx, -1, ts, AVSEEK_FLAG_BACKWARD) };
        if !self.video_bsf.is_null() {
            unsafe { (self.api.av_bsf_flush)(self.video_bsf) };
        }
        if rc < 0 {
            return Err(format!("av_seek_frame failed ({rc})"));
        }
        Ok(())
    }

    pub(crate) fn peek_packet(&mut self) -> Option<&DemuxedPacket> {
        if self.pending.is_none() {
            self.pending = self.read_next_packet();
        }
        self.pending.as_ref()
    }

    pub(crate) fn advance(&mut self) {
        self.pending = None;
    }

    fn read_next_packet(&mut self) -> Option<DemuxedPacket> {
        loop {
            // SAFETY: reads next demuxed packet into `self.packet`.
            let rc = unsafe { (self.api.av_read_frame)(self.fmt_ctx, self.packet) };
            if rc < 0 {
                return None;
            }
            let head = unsafe { &*(self.packet.cast::<AvPacketHead>()) };
            let stream_index = usize::try_from(head.stream_index).ok()?;
            let is_video = self.video_stream == Some(stream_index);

            let out = if is_video && !self.video_bsf.is_null() {
                let converted = self.run_bsf_packet(stream_index);
                unsafe { (self.api.av_packet_unref)(self.packet) };
                converted
            } else {
                let pkt = extract_packet_copy(head, stream_index, &self.time_bases);
                unsafe { (self.api.av_packet_unref)(self.packet) };
                pkt
            };
            if let Some(packet) = out {
                return Some(packet);
            }
        }
    }

    fn run_bsf_packet(&mut self, stream_index: usize) -> Option<DemuxedPacket> {
        // SAFETY: passes `self.packet` through the initialized Annex-B bitstream filter.
        unsafe {
            if (self.api.av_bsf_send_packet)(self.video_bsf, self.packet) < 0 {
                return None;
            }
            if (self.api.av_bsf_receive_packet)(self.video_bsf, self.bsf_packet) < 0 {
                return None;
            }
            let head = &*(self.bsf_packet.cast::<AvPacketHead>());
            let pkt = extract_packet_copy(head, stream_index, &self.time_bases);
            (self.api.av_packet_unref)(self.bsf_packet);
            pkt
        }
    }
}

fn extract_packet_copy(
    head: &AvPacketHead,
    stream_index: usize,
    time_bases: &[AvRational],
) -> Option<DemuxedPacket> {
    if head.data.is_null() || head.size <= 0 {
        return None;
    }
    let size = usize::try_from(head.size).ok()?;
    // SAFETY: `head.data` has `size` valid bytes until `av_packet_unref`.
    let bytes = unsafe { std::slice::from_raw_parts(head.data, size) };
    let raw_ts = if head.pts != AV_NOPTS_VALUE {
        head.pts
    } else if head.dts != AV_NOPTS_VALUE {
        head.dts
    } else {
        0
    };
    let tb = time_bases
        .get(stream_index)
        .copied()
        .unwrap_or(AvRational { num: 1, den: 1_000 });
    let pts = rational_to_duration(raw_ts, tb);
    Some(DemuxedPacket {
        stream_index,
        pts,
        is_keyframe: (head.flags & AV_PKT_FLAG_KEY) != 0,
        data: bytes.to_vec(),
    })
}

fn rational_to_duration(ts: i64, tb: AvRational) -> Duration {
    if ts <= 0 || tb.num <= 0 || tb.den <= 0 {
        return Duration::ZERO;
    }
    let secs = (ts as f64) * (f64::from(tb.num) / f64::from(tb.den));
    if secs.is_finite() && secs > 0.0 {
        Duration::from_secs_f64(secs)
    } else {
        Duration::ZERO
    }
}

pub(crate) struct AudioDecoder {
    api: &'static FfmpegApi,
    ctx: *mut c_void,
    packet: *mut c_void,
    frame: *mut c_void,
    sample_rate: u32,
    channels: usize,
}

// SAFETY: `AudioDecoder` is used exclusively on one player worker thread.
unsafe impl Send for AudioDecoder {}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        // SAFETY: releases owned FFmpeg audio decoder allocations.
        unsafe {
            (self.api.av_frame_free)(&mut self.frame);
            (self.api.av_packet_free)(&mut self.packet);
            (self.api.avcodec_free_context)(&mut self.ctx);
        }
    }
}

impl AudioDecoder {
    fn open(api: &'static FfmpegApi, par: *const c_void) -> Result<Self, String> {
        let codec_id = unsafe { *(par.cast::<u32>().add(1)) };
        // SAFETY: opens audio decoder from stream `AVCodecParameters`.
        let decoder = unsafe { (api.avcodec_find_decoder)(codec_id) };
        if decoder.is_null() {
            return Err(format!("no libavcodec audio decoder for codec_id {codec_id}"));
        }
        let mut ctx = unsafe { (api.avcodec_alloc_context3)(decoder) };
        if ctx.is_null() {
            return Err("avcodec_alloc_context3 failed for audio".into());
        }
        if unsafe { (api.avcodec_parameters_to_context)(ctx, par) } < 0
            || unsafe { (api.avcodec_open2)(ctx, decoder, ptr::null_mut()) } < 0
        {
            unsafe { (api.avcodec_free_context)(&mut ctx) };
            return Err("failed to open libavcodec audio decoder".into());
        }
        let sample_rate = read_opt_u32(api, ctx, c"ar").unwrap_or(48_000);
        let channels = read_opt_channels(api, ctx).unwrap_or(2);
        let packet = unsafe { (api.av_packet_alloc)() };
        let frame = unsafe { (api.av_frame_alloc)() };
        if packet.is_null() || frame.is_null() {
            unsafe {
                let mut f = frame;
                let mut p = packet;
                (api.av_frame_free)(&mut f);
                (api.av_packet_free)(&mut p);
                (api.avcodec_free_context)(&mut ctx);
            }
            return Err("failed to allocate audio packet/frame".into());
        }
        Ok(Self {
            api,
            ctx,
            packet,
            frame,
            sample_rate,
            channels,
        })
    }

    pub(crate) fn send_packet(&mut self, data: &[u8]) -> bool {
        let head = unsafe { &mut *(self.packet.cast::<AvPacketHead>()) };
        head.data = data.as_ptr().cast_mut();
        head.size = c_int::try_from(data.len()).unwrap_or(0);
        head.pts = AV_NOPTS_VALUE;
        head.dts = AV_NOPTS_VALUE;
        // SAFETY: `avcodec_send_packet` reads `data` synchronously before returning.
        let rc = unsafe { (self.api.avcodec_send_packet)(self.ctx, self.packet) };
        head.data = ptr::null_mut();
        head.size = 0;
        rc != AVERROR_EAGAIN
    }

    pub(crate) fn send_eos(&mut self) {
        // SAFETY: passing NULL flushes the audio decoder.
        unsafe {
            let _: c_int = (self.api.avcodec_send_packet)(self.ctx, ptr::null());
        }
    }

    pub(crate) fn receive_pcm(&mut self) -> Option<DecodedAudio> {
        // SAFETY: receives decoded audio frame into `self.frame`.
        let rc = unsafe { (self.api.avcodec_receive_frame)(self.ctx, self.frame) };
        if rc < 0 {
            return None;
        }
        let head = unsafe { &*(self.frame.cast::<AvFrameHead>()) };
        let nb_samples = usize::try_from(head.nb_samples).unwrap_or(0);
        let channels = read_opt_channels(self.api, self.ctx).unwrap_or(self.channels);
        let sample_rate = read_opt_u32(self.api, self.ctx, c"ar").unwrap_or(self.sample_rate);
        self.channels = channels;
        self.sample_rate = sample_rate;
        let samples = unsafe { convert_audio_frame(head, nb_samples, channels) };
        unsafe { (self.api.av_frame_unref)(self.frame) };
        (!samples.is_empty()).then_some(DecodedAudio {
            samples,
            sample_rate,
            channels,
        })
    }

    pub(crate) fn flush(&mut self) {
        // SAFETY: flushes buffered audio samples after seek.
        unsafe { (self.api.avcodec_flush_buffers)(self.ctx) };
    }
}

unsafe fn convert_audio_frame(head: &AvFrameHead, nb_samples: usize, channels: usize) -> Vec<f32> {
    if nb_samples == 0 || channels == 0 {
        return Vec::new();
    }
    let total = nb_samples.saturating_mul(channels);
    let mut out = Vec::with_capacity(total);
    let planes = if !head.extended_data.is_null() {
        head.extended_data
    } else {
        head.data.as_ptr().cast_mut()
    };
    let p0 = unsafe { *planes };
    if p0.is_null() {
        return out;
    }
    match head.format {
        AV_SAMPLE_FMT_FLT => {
            let src = unsafe { std::slice::from_raw_parts(p0.cast::<f32>(), total) };
            out.extend_from_slice(src);
        }
        AV_SAMPLE_FMT_S16 => {
            let src = unsafe { std::slice::from_raw_parts(p0.cast::<i16>(), total) };
            out.extend(src.iter().map(|&s| f32::from(s) / 32_768.0));
        }
        AV_SAMPLE_FMT_S32 => {
            let src = unsafe { std::slice::from_raw_parts(p0.cast::<i32>(), total) };
            out.extend(src.iter().map(|&s| (s as f32) / 2_147_483_648.0));
        }
        AV_SAMPLE_FMT_U8 => {
            let src = unsafe { std::slice::from_raw_parts(p0, total) };
            out.extend(src.iter().map(|&s| (f32::from(s) - 128.0) / 128.0));
        }
        AV_SAMPLE_FMT_DBL => {
            let src = unsafe { std::slice::from_raw_parts(p0.cast::<f64>(), total) };
            out.extend(src.iter().map(|&s| s as f32));
        }
        AV_SAMPLE_FMT_FLTP | AV_SAMPLE_FMT_S16P | AV_SAMPLE_FMT_S32P | AV_SAMPLE_FMT_U8P
        | AV_SAMPLE_FMT_DBLP => {
            out.resize(total, 0.0);
            for ch in 0..channels {
                let plane = unsafe { *planes.add(ch) };
                if plane.is_null() {
                    continue;
                }
                for frame in 0..nb_samples {
                    let sample = match head.format {
                        AV_SAMPLE_FMT_FLTP => unsafe { *plane.cast::<f32>().add(frame) },
                        AV_SAMPLE_FMT_S16P => {
                            f32::from(unsafe { *plane.cast::<i16>().add(frame) }) / 32_768.0
                        }
                        AV_SAMPLE_FMT_S32P => {
                            (unsafe { *plane.cast::<i32>().add(frame) } as f32) / 2_147_483_648.0
                        }
                        AV_SAMPLE_FMT_U8P => {
                            (f32::from(unsafe { *plane.add(frame) }) - 128.0) / 128.0
                        }
                        AV_SAMPLE_FMT_DBLP => unsafe { *plane.cast::<f64>().add(frame) } as f32,
                        _ => 0.0,
                    };
                    out[frame * channels + ch] = sample;
                }
            }
        }
        _ => {}
    }
    out
}

#[derive(Default)]
struct CodecProps {
    width: u32,
    height: u32,
    bitrate: Option<u32>,
    color: Option<VideoColorInfo>,
    sample_rate: Option<u32>,
    channels: Option<usize>,
}

fn probe_codec_props(api: &'static FfmpegApi, par: *const c_void) -> CodecProps {
    // SAFETY: copies parameters into a temporary AVCodecContext so fields can be queried via
    // `av_opt_get_int` without relying on version-dependent struct offsets.
    let mut ctx = unsafe { (api.avcodec_alloc_context3)(ptr::null()) };
    if ctx.is_null() {
        return CodecProps::default();
    }
    let ok = unsafe { (api.avcodec_parameters_to_context)(ctx, par) } >= 0;
    let props = if ok {
        let width = read_opt_u32(api, ctx, c"width").unwrap_or(0);
        let height = read_opt_u32(api, ctx, c"height").unwrap_or(0);
        let bitrate = read_opt_u32(api, ctx, c"b");
        let color = read_opt_color(api, ctx);
        let sample_rate = read_opt_u32(api, ctx, c"ar");
        let channels = read_opt_channels(api, ctx);
        CodecProps {
            width,
            height,
            bitrate,
            color,
            sample_rate,
            channels,
        }
    } else {
        CodecProps::default()
    };
    unsafe { (api.avcodec_free_context)(&mut ctx) };
    props
}

fn read_opt_i64(api: &'static FfmpegApi, obj: *mut c_void, name: &CStr) -> Option<i64> {
    let mut val: i64 = 0;
    // SAFETY: `obj` is an AVClass-backed FFmpeg object and `name` is NUL-terminated.
    let rc = unsafe { (api.av_opt_get_int)(obj, name.as_ptr(), AV_OPT_SEARCH_CHILDREN, &mut val) };
    (rc >= 0).then_some(val)
}

fn read_opt_u32(api: &'static FfmpegApi, obj: *mut c_void, name: &CStr) -> Option<u32> {
    read_opt_i64(api, obj, name)
        .filter(|&v| v > 0)
        .and_then(|v| u32::try_from(v).ok())
}

fn read_opt_channels(api: &'static FfmpegApi, ctx: *mut c_void) -> Option<usize> {
    if let Some(ch) = read_opt_u32(api, ctx, c"ac") {
        return usize::try_from(ch).ok();
    }
    let mut out: *mut u8 = ptr::null_mut();
    // SAFETY: queries `ch_layout` string (e.g. "stereo", "mono", "5.1", "2 channels").
    let rc = unsafe { (api.av_opt_get)(ctx, c"ch_layout".as_ptr(), AV_OPT_SEARCH_CHILDREN, &mut out) };
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
    if primaries.is_none() && range.is_none() && transfer.is_none() {
        return None;
    }
    Some(VideoColorInfo {
        primaries: primaries.unwrap_or(ColorPrimaries::Bt709),
        range: range.unwrap_or(ColorRange::Limited),
        transfer: transfer.unwrap_or(ColorTransfer::Sdr),
    })
}

fn codec_name_of(api: &'static FfmpegApi, codec_id: u32) -> String {
    // SAFETY: `avcodec_get_name` always returns a valid static NUL-terminated string.
    let ptr = unsafe { (api.avcodec_get_name)(codec_id) };
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

fn video_codec_from_name(name: &str) -> Option<VideoCodec> {
    match name {
        "h264" => Some(VideoCodec::H264),
        "hevc" => Some(VideoCodec::Hevc),
        "vp9" => Some(VideoCodec::Vp9),
        "av1" => Some(VideoCodec::Av1),
        _ => None,
    }
}

fn read_stream_metadata(api: &'static FfmpegApi, stream: *mut c_void, key: &CStr) -> Option<String> {
    // SAFETY: `stream_metadata` is the verified offset of `AVStream::metadata`.
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

fn rotation_from_degrees(degrees: i32) -> VideoRotation {
    match degrees.rem_euclid(360) {
        90 => VideoRotation::Degrees90,
        180 => VideoRotation::Degrees180,
        270 => VideoRotation::Degrees270,
        _ => VideoRotation::Degrees0,
    }
}

pub(crate) fn video_size_of(info: &VideoTrackInfo) -> VideoSize {
    VideoSize::new(info.width, info.height)
}
