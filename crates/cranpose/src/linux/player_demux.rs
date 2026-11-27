//! FFmpeg (`libavformat` + `libavcodec`) container demuxer, Annex-B bitstream filter, and PCM
//! audio decoder for Linux [`Player`](crate::Player).
#![expect(
    unsafe_code,
    reason = "C ABI FFI calls into runtime-loaded Linux libavformat, libavcodec, and libavutil"
)]

use std::{
    ffi::{CString, c_int, c_void},
    path::Path,
    ptr,
    sync::Arc,
    time::Duration,
};

use cranpose_services::{TrackEntry, TrackFormat, Tracks, VideoCodec, VideoRotation};

pub(crate) use super::player_ffmpeg::avformat_available;
use super::player_ffmpeg::{
    AV_NOPTS_VALUE, AV_PKT_FLAG_KEY, AV_SAMPLE_FMT_DBL, AV_SAMPLE_FMT_DBLP, AV_SAMPLE_FMT_FLT,
    AV_SAMPLE_FMT_FLTP, AV_SAMPLE_FMT_S16, AV_SAMPLE_FMT_S16P, AV_SAMPLE_FMT_S32,
    AV_SAMPLE_FMT_S32P, AV_SAMPLE_FMT_U8, AV_SAMPLE_FMT_U8P, AV_TIME_BASE, AVERROR_EAGAIN,
    AVERROR_EOF, AVIO_BUF_SIZE, AVMEDIA_TYPE_AUDIO, AVMEDIA_TYPE_VIDEO, AVSEEK_FLAG_BACKWARD,
    AVSEEK_SIZE, AvFrameHead, AvPacketHead, AvRational, FfmpegApi, codec_name_of, ffmpeg_api,
    probe_codec_props, read_opt_channels, read_opt_u32, read_stream_metadata,
    rotation_from_degrees, video_codec_from_name,
};
use crate::player_stream::HttpRangeReader;

#[derive(Clone)]
pub(crate) struct VideoTrackInfo {
    pub(crate) stream_index: usize,
    pub(crate) spec: crate::player_demux_engine::VideoDecoderSpec,
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
    Memory {
        bytes: Arc<[u8]>,
        pos: usize,
    },
    Http {
        reader: Box<HttpRangeReader>,
        pos: u64,
    },
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
        CustomIo::Http { reader, pos } => match reader.read_at(*pos, dst) {
            Ok(0) => return AVERROR_EOF,
            Ok(n) => {
                *pos = pos.saturating_add(n as u64);
                n
            }
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
            CustomIo::Http { reader, .. } => reader
                .size()
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
        CustomIo::Http { reader, pos } => {
            let base = match origin {
                libc::SEEK_SET => 0_i128,
                libc::SEEK_CUR => i128::from(*pos),
                libc::SEEK_END => i128::from(reader.size().unwrap_or(0)),
                _ => return -1,
            };
            let target = base + i128::from(offset);
            if target < 0 {
                return -1;
            }
            let next = u64::try_from(target).unwrap_or(u64::MAX);
            *pos = next;
            i64::try_from(next).unwrap_or(-1)
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
        let api =
            ffmpeg_api().ok_or_else(|| "libavformat is unavailable on this host".to_owned())?;
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
        Self::from_custom_io(CustomIo::Http {
            reader: Box::new(reader),
            pos: 0,
        })
    }

    fn from_custom_io(io: CustomIo) -> Result<Self, String> {
        let api =
            ffmpeg_api().ok_or_else(|| "libavformat is unavailable on this host".to_owned())?;
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
            let tb =
                self.stream_ptr(idx)
                    .map_or(AvRational { num: 1, den: 1_000 }, |stream| unsafe {
                        ptr::read_unaligned(
                            stream
                                .cast::<u8>()
                                .add(self.api.layout.stream_time_base)
                                .cast::<AvRational>(),
                        )
                    });
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
                        spec: crate::player_demux_engine::VideoDecoderSpec {
                            codec,
                            width: props.width,
                            height: props.height,
                            rotation,
                            color: props.color,
                        },
                    });
                    self.setup_video_bsf(idx, codec, par);
                }
                let selection = if selected {
                    TrackSelection::Selected
                } else {
                    TrackSelection::Unselected
                };
                video_entries.push(build_video_track_entry(
                    idx, codec, codec_name, language, &props, selection,
                ));
            } else if codec_type == AVMEDIA_TYPE_AUDIO {
                let selection = if audio_tracks.is_empty() {
                    TrackSelection::Selected
                } else {
                    TrackSelection::Unselected
                };
                audio_tracks.push(AudioTrackInfo {
                    stream_index: idx,
                    language: language.clone(),
                });
                audio_entries.push(build_audio_track_entry(
                    idx, codec_name, language, &props, selection,
                ));
            }
        }

        DemuxedTracks {
            video,
            audio_tracks,
            duration: self.container_duration(),
            tracks: crate::player_demux_engine::build_video_audio_tracks(
                video_entries,
                audio_entries,
            ),
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
            let tb = self
                .time_bases
                .get(idx)
                .copied()
                .unwrap_or(AvRational { num: 1, den: 1_000 });
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
            let par_in = *(bsf
                .cast::<u8>()
                .add(3 * size_of::<*const c_void>())
                .cast::<*mut c_void>());
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
            return Err(format!(
                "no libavcodec audio decoder for codec_id {codec_id}"
            ));
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
    let planes = if !head.extended_data.is_null() {
        head.extended_data
    } else {
        head.data.as_ptr().cast_mut()
    };
    let p0 = unsafe { *planes };
    if p0.is_null() {
        return Vec::new();
    }
    match head.format {
        AV_SAMPLE_FMT_FLT | AV_SAMPLE_FMT_S16 | AV_SAMPLE_FMT_S32 | AV_SAMPLE_FMT_U8
        | AV_SAMPLE_FMT_DBL => unsafe { convert_packed_audio(head.format, p0, total) },
        AV_SAMPLE_FMT_FLTP | AV_SAMPLE_FMT_S16P | AV_SAMPLE_FMT_S32P | AV_SAMPLE_FMT_U8P
        | AV_SAMPLE_FMT_DBLP => unsafe {
            convert_planar_audio(head.format, planes, nb_samples, channels)
        },
        _ => Vec::new(),
    }
}

unsafe fn convert_packed_audio(format: c_int, p0: *const u8, total: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(total);
    match format {
        AV_SAMPLE_FMT_FLT => {
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(p0.cast::<f32>(), total) });
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
        _ => {}
    }
    out
}

unsafe fn convert_planar_audio(
    format: c_int,
    planes: *mut *mut u8,
    nb_samples: usize,
    channels: usize,
) -> Vec<f32> {
    let mut out = vec![0.0_f32; nb_samples.saturating_mul(channels)];
    for ch in 0..channels {
        let plane = unsafe { *planes.add(ch) };
        if plane.is_null() {
            continue;
        }
        for frame in 0..nb_samples {
            out[frame * channels + ch] = unsafe { read_planar_sample(format, plane, frame) };
        }
    }
    out
}

unsafe fn read_planar_sample(format: c_int, plane: *const u8, frame: usize) -> f32 {
    match format {
        AV_SAMPLE_FMT_FLTP => unsafe { *plane.cast::<f32>().add(frame) },
        AV_SAMPLE_FMT_S16P => f32::from(unsafe { *plane.cast::<i16>().add(frame) }) / 32_768.0,
        AV_SAMPLE_FMT_S32P => (unsafe { *plane.cast::<i32>().add(frame) } as f32) / 2_147_483_648.0,
        AV_SAMPLE_FMT_U8P => (f32::from(unsafe { *plane.add(frame) }) - 128.0) / 128.0,
        AV_SAMPLE_FMT_DBLP => (unsafe { *plane.cast::<f64>().add(frame) }) as f32,
        _ => 0.0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackSelection {
    Selected,
    Unselected,
}

fn build_video_track_entry(
    idx: usize,
    codec: VideoCodec,
    codec_name: String,
    language: Option<String>,
    props: &super::player_ffmpeg::CodecProps,
    selection: TrackSelection,
) -> TrackEntry {
    TrackEntry {
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
        selected: selection == TrackSelection::Selected,
    }
}

fn build_audio_track_entry(
    idx: usize,
    codec_name: String,
    language: Option<String>,
    props: &super::player_ffmpeg::CodecProps,
    selection: TrackSelection,
) -> TrackEntry {
    TrackEntry {
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
        selected: selection == TrackSelection::Selected,
    }
}
