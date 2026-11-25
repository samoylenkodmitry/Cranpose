//! Android NDK `AMediaExtractor`, `AMediaDataSource` (API 28+ via `dlsym`), and audio
//! `AMediaCodec` wrapper for zero-temp-file local, `content://`, in-memory, HTTP range, and
//! HLS/DASH demuxing and PCM audio decoding.
#![expect(
    unsafe_code,
    reason = "FFI boundary to Android libmediandk.so (AMediaExtractor, AMediaDataSource, AMediaCodec)"
)]

use std::{
    ffi::{CStr, CString, c_char, c_void},
    fs::File,
    os::fd::AsRawFd,
    ptr::{self, NonNull},
    sync::{Arc, OnceLock},
    time::Duration,
};

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, TrackEntry, TrackFormat, TrackGroup, TrackType,
    Tracks, VideoCodec, VideoColorInfo, VideoRotation, VideoSize,
};
use ndk_sys::{
    AMediaCodec, AMediaCodec_configure, AMediaCodec_createDecoderByType, AMediaCodec_delete,
    AMediaCodec_dequeueInputBuffer, AMediaCodec_dequeueOutputBuffer, AMediaCodec_flush,
    AMediaCodec_getInputBuffer, AMediaCodec_getOutputBuffer, AMediaCodec_getOutputFormat,
    AMediaCodec_queueInputBuffer, AMediaCodec_releaseOutputBuffer, AMediaCodec_start,
    AMediaCodec_stop, AMediaCodecBufferInfo, AMediaDataSource, AMediaDataSourceClose,
    AMediaDataSourceGetSize, AMediaDataSourceReadAt, AMediaExtractor, AMediaExtractor_advance,
    AMediaExtractor_delete, AMediaExtractor_getCachedDuration, AMediaExtractor_getSampleFlags,
    AMediaExtractor_getSampleTime, AMediaExtractor_getSampleTrackIndex,
    AMediaExtractor_getTrackCount, AMediaExtractor_getTrackFormat, AMediaExtractor_new,
    AMediaExtractor_readSampleData, AMediaExtractor_seekTo, AMediaExtractor_selectTrack,
    AMediaExtractor_setDataSource, AMediaExtractor_setDataSourceFd, AMediaExtractor_unselectTrack,
    AMediaFormat, AMediaFormat_delete, AMediaFormat_getBuffer, AMediaFormat_getInt32,
    AMediaFormat_getInt64, AMediaFormat_getString, SeekMode, media_status_t, off64_t,
};

use crate::player_stream::HttpRangeReader;

const SAMPLE_FLAG_SYNC: u32 = 1;
const CODEC_FLAG_EOS: u32 = 4;
const INFO_OUTPUT_FORMAT_CHANGED: isize = -2;
const INFO_OUTPUT_BUFFERS_CHANGED: isize = -3;
const PCM_ENCODING_FLOAT: i32 = 4;

type DataSourceNewFn = unsafe extern "C" fn() -> *mut AMediaDataSource;
type DataSourceDeleteFn = unsafe extern "C" fn(*mut AMediaDataSource);
type DataSourceSetUserdataFn = unsafe extern "C" fn(*mut AMediaDataSource, *mut c_void);
type DataSourceSetReadAtFn = unsafe extern "C" fn(*mut AMediaDataSource, AMediaDataSourceReadAt);
type DataSourceSetGetSizeFn = unsafe extern "C" fn(*mut AMediaDataSource, AMediaDataSourceGetSize);
type DataSourceSetCloseFn = unsafe extern "C" fn(*mut AMediaDataSource, AMediaDataSourceClose);
type SetDataSourceCustomFn =
    unsafe extern "C" fn(*mut AMediaExtractor, *mut AMediaDataSource) -> media_status_t;

struct DataSourceSymbols {
    new: DataSourceNewFn,
    delete: DataSourceDeleteFn,
    set_userdata: DataSourceSetUserdataFn,
    set_read_at: DataSourceSetReadAtFn,
    set_get_size: DataSourceSetGetSizeFn,
    set_close: DataSourceSetCloseFn,
    set_custom: SetDataSourceCustomFn,
}

fn data_source_symbols() -> Option<&'static DataSourceSymbols> {
    static SYMS: OnceLock<Option<DataSourceSymbols>> = OnceLock::new();
    SYMS.get_or_init(|| {
        // SAFETY: libmediandk.so is an Android system library; symbols added in API 28 are
        // resolved dynamically so apps linked against API 24 still load cleanly.
        let lib = unsafe { libc::dlopen(c"libmediandk.so".as_ptr(), libc::RTLD_LAZY) };
        if lib.is_null() {
            return None;
        }
        unsafe {
            let new = resolve_sym(lib, c"AMediaDataSource_new")?;
            let delete = resolve_sym(lib, c"AMediaDataSource_delete")?;
            let set_userdata = resolve_sym(lib, c"AMediaDataSource_setUserdata")?;
            let set_read_at = resolve_sym(lib, c"AMediaDataSource_setReadAt")?;
            let set_get_size = resolve_sym(lib, c"AMediaDataSource_setGetSize")?;
            let set_close = resolve_sym(lib, c"AMediaDataSource_setClose")?;
            let set_custom = resolve_sym(lib, c"AMediaExtractor_setDataSourceCustom")?;
            Some(DataSourceSymbols {
                new: std::mem::transmute::<*mut c_void, DataSourceNewFn>(new),
                delete: std::mem::transmute::<*mut c_void, DataSourceDeleteFn>(delete),
                set_userdata: std::mem::transmute::<*mut c_void, DataSourceSetUserdataFn>(
                    set_userdata,
                ),
                set_read_at: std::mem::transmute::<*mut c_void, DataSourceSetReadAtFn>(set_read_at),
                set_get_size: std::mem::transmute::<*mut c_void, DataSourceSetGetSizeFn>(
                    set_get_size,
                ),
                set_close: std::mem::transmute::<*mut c_void, DataSourceSetCloseFn>(set_close),
                set_custom: std::mem::transmute::<*mut c_void, SetDataSourceCustomFn>(set_custom),
            })
        }
    })
    .as_ref()
}

unsafe fn resolve_sym(lib: *mut c_void, name: &CStr) -> Option<*mut c_void> {
    let ptr = unsafe { libc::dlsym(lib, name.as_ptr()) };
    (!ptr.is_null()).then_some(ptr)
}

enum CustomSource {
    Memory(Arc<[u8]>),
    Http(Box<HttpRangeReader>),
}

struct CustomDataSource {
    symbols: &'static DataSourceSymbols,
    raw: NonNull<AMediaDataSource>,
    userdata: *mut CustomSource,
}

impl CustomDataSource {
    fn new(source: CustomSource) -> Result<Self, String> {
        let symbols = data_source_symbols()
            .ok_or_else(|| "AMediaDataSource requires Android 9 (API 28) or newer".to_owned())?;
        // SAFETY: `AMediaDataSource_new` allocates a new native data source handle.
        let raw = NonNull::new(unsafe { (symbols.new)() })
            .ok_or_else(|| "AMediaDataSource_new returned null".to_owned())?;
        let userdata = Box::into_raw(Box::new(source));
        // SAFETY: `raw` is live and callbacks match NDK's `AMediaDataSource` ABI.
        unsafe {
            (symbols.set_userdata)(raw.as_ptr(), userdata.cast());
            (symbols.set_read_at)(raw.as_ptr(), Some(custom_read_at));
            (symbols.set_get_size)(raw.as_ptr(), Some(custom_get_size));
            (symbols.set_close)(raw.as_ptr(), Some(custom_close));
        }
        Ok(Self {
            symbols,
            raw,
            userdata,
        })
    }
}

impl Drop for CustomDataSource {
    fn drop(&mut self) {
        // SAFETY: extractor owning references to this data source is dropped before this struct.
        unsafe {
            (self.symbols.delete)(self.raw.as_ptr());
            drop(Box::from_raw(self.userdata));
        }
    }
}

unsafe extern "C" fn custom_read_at(
    userdata: *mut c_void,
    offset: off64_t,
    buffer: *mut c_void,
    size: usize,
) -> isize {
    if userdata.is_null() || buffer.is_null() || offset < 0 {
        return -1;
    }
    if size == 0 {
        return 0;
    }
    let off = offset.cast_unsigned();
    // SAFETY: `userdata` points to our live `CustomSource` and `buffer` has `size` writable bytes.
    let source = unsafe { &mut *userdata.cast::<CustomSource>() };
    let out = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size) };
    match source {
        CustomSource::Memory(bytes) => {
            let Ok(start) = usize::try_from(off) else {
                return 0;
            };
            if start >= bytes.len() {
                return 0;
            }
            let count = (bytes.len() - start).min(size);
            out[..count].copy_from_slice(&bytes[start..start + count]);
            isize::try_from(count).unwrap_or(-1)
        }
        CustomSource::Http(reader) => match reader.read_at(off, out) {
            Ok(count) => isize::try_from(count).unwrap_or(-1),
            Err(_) => -1,
        },
    }
}

unsafe extern "C" fn custom_get_size(userdata: *mut c_void) -> isize {
    if userdata.is_null() {
        return -1;
    }
    // SAFETY: `userdata` points to our live `CustomSource`.
    let source = unsafe { &*userdata.cast::<CustomSource>() };
    match source {
        CustomSource::Memory(bytes) => isize::try_from(bytes.len()).unwrap_or(-1),
        CustomSource::Http(reader) => reader
            .size()
            .and_then(|s| isize::try_from(s).ok())
            .unwrap_or(-1),
    }
}

unsafe extern "C" fn custom_close(_userdata: *mut c_void) {}

#[derive(Clone, Debug)]
pub(crate) struct VideoTrackInfo {
    pub(crate) track_index: usize,
    pub(crate) codec: VideoCodec,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rotation: VideoRotation,
    pub(crate) color: Option<VideoColorInfo>,
    pub(crate) csd: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct AudioTrackInfo {
    pub(crate) track_index: usize,
    pub(crate) language: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ExtractedTracks {
    pub(crate) video: Option<VideoTrackInfo>,
    pub(crate) audio_tracks: Vec<AudioTrackInfo>,
    pub(crate) duration: Option<Duration>,
    pub(crate) tracks: Tracks,
}

pub(crate) struct DemuxedSample {
    pub(crate) track_index: usize,
    pub(crate) pts: Duration,
    pub(crate) is_sync: bool,
    pub(crate) size: usize,
}

enum SourceBacking {
    File { _file: File },
    Custom { _source: CustomDataSource },
    Uri,
}

pub(crate) struct MediaExtractor {
    raw: NonNull<AMediaExtractor>,
    // Field drop order matters: `raw` MUST be dropped before `_backing`, handled in `Drop`.
    _backing: SourceBacking,
    selected_tracks: Vec<usize>,
}

impl MediaExtractor {
    pub(crate) fn from_file(file: File) -> Result<Self, String> {
        let raw = alloc_extractor()?;
        let length = file
            .metadata()
            .ok()
            .and_then(|m| i64::try_from(m.len()).ok())
            .unwrap_or(i64::MAX);
        // SAFETY: `raw` is live and `file` remains open for the lifetime of `MediaExtractor`.
        let status =
            unsafe { AMediaExtractor_setDataSourceFd(raw.as_ptr(), file.as_raw_fd(), 0, length) };
        if status != media_status_t::AMEDIA_OK {
            unsafe {
                AMediaExtractor_delete(raw.as_ptr());
            }
            return Err(format!(
                "AMediaExtractor_setDataSourceFd failed ({status:?})"
            ));
        }
        Ok(Self {
            raw,
            _backing: SourceBacking::File { _file: file },
            selected_tracks: Vec::new(),
        })
    }

    pub(crate) fn from_bytes(bytes: Arc<[u8]>) -> Result<Self, String> {
        Self::from_custom(CustomSource::Memory(bytes))
    }

    pub(crate) fn from_http(reader: HttpRangeReader) -> Result<Self, String> {
        Self::from_custom(CustomSource::Http(Box::new(reader)))
    }

    pub(crate) fn from_uri(uri: &str) -> Result<Self, String> {
        let raw = alloc_extractor()?;
        let c_uri = CString::new(uri).map_err(|_| "URI contains interior NUL byte".to_owned())?;
        // SAFETY: `raw` and `c_uri` are valid.
        let status = unsafe { AMediaExtractor_setDataSource(raw.as_ptr(), c_uri.as_ptr()) };
        if status != media_status_t::AMEDIA_OK {
            unsafe {
                AMediaExtractor_delete(raw.as_ptr());
            }
            return Err(format!("AMediaExtractor_setDataSource failed ({status:?})"));
        }
        Ok(Self {
            raw,
            _backing: SourceBacking::Uri,
            selected_tracks: Vec::new(),
        })
    }

    fn from_custom(source: CustomSource) -> Result<Self, String> {
        let raw = alloc_extractor()?;
        let custom = match CustomDataSource::new(source) {
            Ok(c) => c,
            Err(err) => {
                unsafe {
                    AMediaExtractor_delete(raw.as_ptr());
                }
                return Err(err);
            }
        };
        // SAFETY: both `raw` and `custom.raw` are live; `raw` is deleted before `custom` in `Drop`.
        let status = unsafe { (custom.symbols.set_custom)(raw.as_ptr(), custom.raw.as_ptr()) };
        if status != media_status_t::AMEDIA_OK {
            unsafe {
                AMediaExtractor_delete(raw.as_ptr());
            }
            return Err(format!(
                "AMediaExtractor_setDataSourceCustom failed ({status:?})"
            ));
        }
        Ok(Self {
            raw,
            _backing: SourceBacking::Custom { _source: custom },
            selected_tracks: Vec::new(),
        })
    }

    pub(crate) fn inspect_tracks(&self) -> ExtractedTracks {
        // SAFETY: `self.raw` is a live `AMediaExtractor`.
        let count = unsafe { AMediaExtractor_getTrackCount(self.raw.as_ptr()) };
        let mut video: Option<VideoTrackInfo> = None;
        let mut video_entries = Vec::new();
        let mut audio_tracks = Vec::new();
        let mut audio_entries = Vec::new();
        let mut max_duration_us: i64 = 0;

        for idx in 0..count {
            // SAFETY: `idx < count` on a live extractor.
            let fmt = unsafe { AMediaExtractor_getTrackFormat(self.raw.as_ptr(), idx) };
            if fmt.is_null() {
                continue;
            }
            if let Some(dur_us) = format_i64(fmt, c"durationUs")
                && dur_us > max_duration_us
            {
                max_duration_us = dur_us;
            }
            let mime = format_string(fmt, c"mime").unwrap_or_default();
            let language = format_string(fmt, c"language").filter(|l| l != "und" && !l.is_empty());
            let bitrate = format_i32(fmt, c"bitrate")
                .filter(|&b| b > 0)
                .map(i32::cast_unsigned);

            if let Some(codec) = video_codec_from_mime(&mime) {
                let (info, entry) =
                    parse_video_track(fmt, idx, codec, mime, language, bitrate, video.is_none());
                if video.is_none() {
                    video = info;
                }
                video_entries.push(entry);
            } else if mime.starts_with("audio/") {
                let (info, entry) =
                    parse_audio_track(fmt, idx, mime, language, bitrate, audio_tracks.is_empty());
                audio_tracks.push(info);
                audio_entries.push(entry);
            }
            // SAFETY: `fmt` was returned by `AMediaExtractor_getTrackFormat`.
            unsafe {
                AMediaFormat_delete(fmt);
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
        let duration =
            (max_duration_us > 0).then(|| Duration::from_micros(max_duration_us.cast_unsigned()));
        ExtractedTracks {
            video,
            audio_tracks,
            duration,
            tracks: Tracks { groups },
        }
    }

    pub(crate) fn select_track(&mut self, index: usize) -> Result<(), String> {
        if self.selected_tracks.contains(&index) {
            return Ok(());
        }
        // SAFETY: `self.raw` is live.
        let status = unsafe { AMediaExtractor_selectTrack(self.raw.as_ptr(), index) };
        if status != media_status_t::AMEDIA_OK {
            return Err(format!(
                "AMediaExtractor_selectTrack({index}) failed ({status:?})"
            ));
        }
        self.selected_tracks.push(index);
        Ok(())
    }

    pub(crate) fn unselect_track(&mut self, index: usize) {
        if let Some(pos) = self.selected_tracks.iter().position(|&t| t == index) {
            // SAFETY: `self.raw` is live.
            unsafe {
                AMediaExtractor_unselectTrack(self.raw.as_ptr(), index);
            }
            self.selected_tracks.remove(pos);
        }
    }

    pub(crate) fn open_audio_decoder(
        &self,
        track_index: usize,
    ) -> Result<AudioCodecDecoder, String> {
        // SAFETY: `self.raw` is live.
        let fmt = unsafe { AMediaExtractor_getTrackFormat(self.raw.as_ptr(), track_index) };
        if fmt.is_null() {
            return Err("AMediaExtractor_getTrackFormat returned null for audio track".into());
        }
        let result = AudioCodecDecoder::from_format(fmt);
        // SAFETY: `fmt` was returned by `AMediaExtractor_getTrackFormat`.
        unsafe {
            AMediaFormat_delete(fmt);
        }
        result
    }

    pub(crate) fn seek_to(&mut self, position: Duration) -> Result<(), String> {
        let us = i64::try_from(position.as_micros()).unwrap_or(i64::MAX);
        // SAFETY: `self.raw` is live.
        let status = unsafe {
            AMediaExtractor_seekTo(
                self.raw.as_ptr(),
                us,
                SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC,
            )
        };
        if status != media_status_t::AMEDIA_OK {
            return Err(format!("AMediaExtractor_seekTo failed ({status:?})"));
        }
        Ok(())
    }

    pub(crate) fn cached_duration(&self) -> Option<Duration> {
        // SAFETY: `self.raw` is live.
        let us = unsafe { AMediaExtractor_getCachedDuration(self.raw.as_ptr()) };
        (us >= 0).then(|| Duration::from_micros(us.cast_unsigned()))
    }

    pub(crate) fn read_sample(&mut self, buf: &mut Vec<u8>) -> Option<DemuxedSample> {
        // SAFETY: `self.raw` is live.
        let track_idx = unsafe { AMediaExtractor_getSampleTrackIndex(self.raw.as_ptr()) };
        if track_idx < 0 {
            return None;
        }
        if buf.len() < 256 * 1024 {
            buf.resize(256 * 1024, 0);
        }
        // SAFETY: `buf` has writable capacity `buf.len()`.
        let read = unsafe {
            AMediaExtractor_readSampleData(self.raw.as_ptr(), buf.as_mut_ptr(), buf.len())
        };
        if read < 0 {
            return None;
        }
        let size = read.cast_unsigned();
        let pts_us = unsafe { AMediaExtractor_getSampleTime(self.raw.as_ptr()) };
        let flags = unsafe { AMediaExtractor_getSampleFlags(self.raw.as_ptr()) };
        unsafe {
            AMediaExtractor_advance(self.raw.as_ptr());
        }
        Some(DemuxedSample {
            track_index: usize::try_from(track_idx).unwrap_or(0),
            pts: Duration::from_micros(pts_us.max(0).cast_unsigned()),
            is_sync: (flags & SAMPLE_FLAG_SYNC) != 0,
            size,
        })
    }
}

impl Drop for MediaExtractor {
    fn drop(&mut self) {
        // SAFETY: delete extractor BEFORE `_backing` (File or CustomDataSource) drops.
        unsafe {
            AMediaExtractor_delete(self.raw.as_ptr());
        }
    }
}

pub(crate) enum DecodedAudio {
    Pcm16 {
        samples: Vec<i16>,
        sample_rate: u32,
        channels: usize,
    },
    PcmFloat {
        samples: Vec<f32>,
        sample_rate: u32,
        channels: usize,
    },
}

pub(crate) struct AudioCodecDecoder {
    raw: NonNull<AMediaCodec>,
    sample_rate: u32,
    channels: usize,
    pcm_float: bool,
}

impl AudioCodecDecoder {
    fn from_format(fmt: *mut AMediaFormat) -> Result<Self, String> {
        let mime = format_string(fmt, c"mime")
            .ok_or_else(|| "audio track missing mime type".to_owned())?;
        let c_mime =
            CString::new(mime.as_str()).map_err(|_| "audio mime contains NUL byte".to_owned())?;
        let sample_rate = format_i32(fmt, c"sample-rate")
            .filter(|&r| r > 0)
            .map_or(48_000, i32::cast_unsigned);
        let channels = format_i32(fmt, c"channel-count")
            .filter(|&c| c > 0)
            .map_or(2, |c| usize::try_from(c).unwrap_or(2));
        // SAFETY: `c_mime` is NUL-terminated and `fmt` is a valid `AMediaFormat`.
        let raw = NonNull::new(unsafe { AMediaCodec_createDecoderByType(c_mime.as_ptr()) })
            .ok_or_else(|| format!("no Android audio decoder for {mime}"))?;
        let cfg = unsafe {
            AMediaCodec_configure(raw.as_ptr(), fmt, ptr::null_mut(), ptr::null_mut(), 0)
        };
        if cfg != media_status_t::AMEDIA_OK {
            unsafe {
                AMediaCodec_delete(raw.as_ptr());
            }
            return Err(format!("audio AMediaCodec_configure failed ({cfg:?})"));
        }
        let started = unsafe { AMediaCodec_start(raw.as_ptr()) };
        if started != media_status_t::AMEDIA_OK {
            unsafe {
                AMediaCodec_delete(raw.as_ptr());
            }
            return Err(format!("audio AMediaCodec_start failed ({started:?})"));
        }
        Ok(Self {
            raw,
            sample_rate,
            channels,
            pcm_float: false,
        })
    }

    pub(crate) fn queue_input(&mut self, data: &[u8], pts: Duration) -> bool {
        self.queue_buffer(data, pts, 0)
    }

    pub(crate) fn queue_eos(&mut self) -> bool {
        self.queue_buffer(&[], Duration::ZERO, CODEC_FLAG_EOS)
    }

    fn queue_buffer(&mut self, data: &[u8], pts: Duration, flags: u32) -> bool {
        // SAFETY: `self.raw` is a started `AMediaCodec`.
        let idx = unsafe { AMediaCodec_dequeueInputBuffer(self.raw.as_ptr(), 2_000) };
        if idx < 0 {
            return false;
        }
        let buf_idx = idx.cast_unsigned();
        let mut cap: usize = 0;
        // SAFETY: `buf_idx` was just dequeued from `self.raw`.
        let ptr = unsafe { AMediaCodec_getInputBuffer(self.raw.as_ptr(), buf_idx, &mut cap) };
        if ptr.is_null() {
            return false;
        }
        let copy_len = data.len().min(cap);
        if copy_len > 0 {
            // SAFETY: `ptr` has `cap` writable bytes and `copy_len <= cap`.
            unsafe {
                ptr::copy_nonoverlapping(data.as_ptr(), ptr, copy_len);
            }
        }
        let pts_us = u64::try_from(pts.as_micros()).unwrap_or(u64::MAX);
        let status = unsafe {
            AMediaCodec_queueInputBuffer(self.raw.as_ptr(), buf_idx, 0, copy_len, pts_us, flags)
        };
        status == media_status_t::AMEDIA_OK
    }

    pub(crate) fn dequeue_output(&mut self) -> Option<DecodedAudio> {
        let mut info = AMediaCodecBufferInfo {
            offset: 0,
            size: 0,
            presentationTimeUs: 0,
            flags: 0,
        };
        // SAFETY: `self.raw` is a started `AMediaCodec`.
        let status = unsafe { AMediaCodec_dequeueOutputBuffer(self.raw.as_ptr(), &mut info, 0) };
        if status == INFO_OUTPUT_FORMAT_CHANGED {
            self.refresh_output_format();
            return None;
        }
        if status == INFO_OUTPUT_BUFFERS_CHANGED || status < 0 {
            return None;
        }
        let buf_idx = status.cast_unsigned();
        let mut cap: usize = 0;
        let ptr = unsafe { AMediaCodec_getOutputBuffer(self.raw.as_ptr(), buf_idx, &mut cap) };
        let decoded = if !ptr.is_null() && info.size > 0 && info.offset >= 0 {
            let offset = usize::try_from(info.offset).unwrap_or(0);
            let size = usize::try_from(info.size).unwrap_or(0);
            if offset.saturating_add(size) <= cap {
                // SAFETY: slice lies inside the dequeued output buffer `[offset..offset+size]`.
                let bytes = unsafe { std::slice::from_raw_parts(ptr.add(offset), size) };
                Some(decode_pcm_bytes(
                    bytes,
                    self.sample_rate,
                    self.channels,
                    self.pcm_float,
                ))
            } else {
                None
            }
        } else {
            None
        };
        // SAFETY: release the dequeued output buffer without rendering to a surface.
        unsafe {
            AMediaCodec_releaseOutputBuffer(self.raw.as_ptr(), buf_idx, false);
        }
        decoded
    }

    pub(crate) fn flush(&mut self) {
        // SAFETY: `self.raw` is a started `AMediaCodec`.
        unsafe {
            AMediaCodec_flush(self.raw.as_ptr());
        }
    }

    fn refresh_output_format(&mut self) {
        // SAFETY: `self.raw` is live; format must be deleted after reading.
        let fmt = unsafe { AMediaCodec_getOutputFormat(self.raw.as_ptr()) };
        if fmt.is_null() {
            return;
        }
        if let Some(rate) = format_i32(fmt, c"sample-rate").filter(|&r| r > 0) {
            self.sample_rate = rate.cast_unsigned();
        }
        if let Some(ch) = format_i32(fmt, c"channel-count").filter(|&c| c > 0) {
            self.channels = usize::try_from(ch).unwrap_or(2);
        }
        self.pcm_float = format_i32(fmt, c"pcm-encoding") == Some(PCM_ENCODING_FLOAT);
        unsafe {
            AMediaFormat_delete(fmt);
        }
    }
}

impl Drop for AudioCodecDecoder {
    fn drop(&mut self) {
        // SAFETY: stop and delete owned audio `AMediaCodec`.
        unsafe {
            AMediaCodec_stop(self.raw.as_ptr());
            AMediaCodec_delete(self.raw.as_ptr());
        }
    }
}

fn decode_pcm_bytes(
    bytes: &[u8],
    sample_rate: u32,
    channels: usize,
    pcm_float: bool,
) -> DecodedAudio {
    if pcm_float {
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&chunk| f32::from_le_bytes(chunk))
            .collect();
        DecodedAudio::PcmFloat {
            samples,
            sample_rate,
            channels,
        }
    } else {
        let samples = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&chunk| i16::from_le_bytes(chunk))
            .collect();
        DecodedAudio::Pcm16 {
            samples,
            sample_rate,
            channels,
        }
    }
}

fn parse_video_track(
    fmt: *mut AMediaFormat,
    idx: usize,
    codec: VideoCodec,
    mime: String,
    language: Option<String>,
    bitrate: Option<u32>,
    selected: bool,
) -> (Option<VideoTrackInfo>, TrackEntry) {
    let width = format_i32(fmt, c"width")
        .filter(|&v| v > 0)
        .map_or(0, i32::cast_unsigned);
    let height = format_i32(fmt, c"height")
        .filter(|&v| v > 0)
        .map_or(0, i32::cast_unsigned);
    let rotation = rotation_from_degrees(format_i32(fmt, c"rotation-degrees").unwrap_or(0));
    let color = read_format_color(fmt);
    let mut csd = Vec::new();
    if let Some(csd0) = format_buffer(fmt, c"csd-0") {
        append_annex_b_csd(&mut csd, csd0);
    }
    if let Some(csd1) = format_buffer(fmt, c"csd-1") {
        append_annex_b_csd(&mut csd, csd1);
    }
    let info = (selected && width > 0 && height > 0).then_some(VideoTrackInfo {
        track_index: idx,
        codec,
        width,
        height,
        rotation,
        color,
        csd,
    });
    let entry = TrackEntry {
        format: TrackFormat {
            id: format!("video-{idx}"),
            label: None,
            language,
            mime_type: Some(mime),
            codecs: None,
            bitrate,
            width: (width > 0).then_some(width),
            height: (height > 0).then_some(height),
            frame_rate: format_i32(fmt, c"frame-rate")
                .filter(|&r| r > 0)
                .map(|r| r as f32),
            channel_count: None,
            sample_rate: None,
        },
        supported: true,
        selected,
    };
    (info, entry)
}

fn parse_audio_track(
    fmt: *mut AMediaFormat,
    idx: usize,
    mime: String,
    language: Option<String>,
    bitrate: Option<u32>,
    selected: bool,
) -> (AudioTrackInfo, TrackEntry) {
    let sample_rate = format_i32(fmt, c"sample-rate")
        .filter(|&r| r > 0)
        .map_or(48_000, i32::cast_unsigned);
    let channel_count = format_i32(fmt, c"channel-count")
        .filter(|&c| c > 0)
        .map_or(2, |c| usize::try_from(c).unwrap_or(2));
    let info = AudioTrackInfo {
        track_index: idx,
        language: language.clone(),
    };
    let entry = TrackEntry {
        format: TrackFormat {
            id: format!("audio-{idx}"),
            label: None,
            language,
            mime_type: Some(mime),
            codecs: None,
            bitrate,
            width: None,
            height: None,
            frame_rate: None,
            channel_count: Some(u16::try_from(channel_count).unwrap_or(2)),
            sample_rate: Some(sample_rate),
        },
        supported: true,
        selected,
    };
    (info, entry)
}

fn alloc_extractor() -> Result<NonNull<AMediaExtractor>, String> {
    // SAFETY: `AMediaExtractor_new` takes no arguments and returns a new handle or null.
    NonNull::new(unsafe { AMediaExtractor_new() })
        .ok_or_else(|| "AMediaExtractor_new returned null".to_owned())
}

fn format_string(fmt: *mut AMediaFormat, key: &CStr) -> Option<String> {
    let mut out: *const c_char = ptr::null();
    // SAFETY: `fmt` is a valid `AMediaFormat` and `key` is NUL-terminated.
    let ok = unsafe { AMediaFormat_getString(fmt, key.as_ptr(), &mut out) };
    if !ok || out.is_null() {
        return None;
    }
    // SAFETY: NDK guarantees `out` is a valid NUL-terminated string owned by `fmt`.
    Some(
        unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn format_i32(fmt: *mut AMediaFormat, key: &CStr) -> Option<i32> {
    let mut out: i32 = 0;
    // SAFETY: `fmt` and `key` are valid.
    unsafe { AMediaFormat_getInt32(fmt, key.as_ptr(), &mut out) }.then_some(out)
}

fn format_i64(fmt: *mut AMediaFormat, key: &CStr) -> Option<i64> {
    let mut out: i64 = 0;
    // SAFETY: `fmt` and `key` are valid.
    unsafe { AMediaFormat_getInt64(fmt, key.as_ptr(), &mut out) }.then_some(out)
}

fn format_buffer<'a>(fmt: *mut AMediaFormat, key: &CStr) -> Option<&'a [u8]> {
    let mut data: *mut c_void = ptr::null_mut();
    let mut size: usize = 0;
    // SAFETY: `fmt` and `key` are valid; returned buffer is borrowed from `fmt`.
    let ok = unsafe { AMediaFormat_getBuffer(fmt, key.as_ptr(), &mut data, &mut size) };
    if !ok || data.is_null() || size == 0 {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) })
}

fn video_codec_from_mime(mime: &str) -> Option<VideoCodec> {
    match mime {
        "video/avc" => Some(VideoCodec::H264),
        "video/hevc" => Some(VideoCodec::Hevc),
        "video/x-vnd.on2.vp9" => Some(VideoCodec::Vp9),
        "video/av01" => Some(VideoCodec::Av1),
        _ => None,
    }
}

fn rotation_from_degrees(degrees: i32) -> VideoRotation {
    match degrees.rem_euclid(360) {
        90 => VideoRotation::Degrees90,
        180 => VideoRotation::Degrees180,
        270 => VideoRotation::Degrees270,
        _ => VideoRotation::Degrees0,
    }
}

fn read_format_color(fmt: *mut AMediaFormat) -> Option<VideoColorInfo> {
    let primaries = match format_i32(fmt, c"color-standard") {
        Some(1) => Some(ColorPrimaries::Bt709),
        Some(2 | 4) => Some(ColorPrimaries::Bt601),
        Some(6) => Some(ColorPrimaries::Bt2020),
        _ => None,
    };
    let range = match format_i32(fmt, c"color-range") {
        Some(1) => Some(ColorRange::Full),
        Some(2) => Some(ColorRange::Limited),
        _ => None,
    };
    let transfer = match format_i32(fmt, c"color-transfer") {
        Some(1 | 3) => Some(ColorTransfer::Sdr),
        Some(6) => Some(ColorTransfer::Pq),
        Some(7) => Some(ColorTransfer::Hlg),
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

fn append_annex_b_csd(dst: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if bytes.starts_with(&[0, 0, 0, 1]) || bytes.starts_with(&[0, 0, 1]) {
        dst.extend_from_slice(bytes);
    } else {
        dst.extend_from_slice(&[0, 0, 0, 1]);
        dst.extend_from_slice(bytes);
    }
}

pub(crate) fn video_size_of(info: &VideoTrackInfo) -> VideoSize {
    VideoSize::new(info.width, info.height)
}
