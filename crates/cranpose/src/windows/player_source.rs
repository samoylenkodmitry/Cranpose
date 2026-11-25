//! Media Foundation `IMFSourceReader` zero-copy video + float PCM audio source for Windows
//! [`Player`](crate::Player).
//!
//! Hardware decoders output directly into D3D11 `NV12` / `P010` surfaces bound through
//! `IMFDXGIDeviceManager` and exported via [`D3d11SurfaceExporter`] into Cranpose's D3D12
//! renderer with zero CPU pixel copies. In-memory bytes and HTTP range streams use a custom
//! `IMFByteStream` with zero temporary files on disk.
#![expect(
    unsafe_code,
    reason = "Media Foundation IMFSourceReader, IMFByteStream, and D3D11/DXGI are Win32 COM APIs"
)]

use std::{
    collections::BTreeMap,
    ffi::c_void,
    path::Path,
    ptr,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, CropRect, D3d11FrameDescriptor,
    D3d11SurfaceExporter, DxgiPixelFormat, FrameInfo, TrackEntry, TrackFormat, TrackGroup,
    TrackType, Tracks, VideoColorInfo, VideoFrame, VideoRotation, VideoSize,
};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, E_NOTIMPL, E_POINTER},
        Graphics::Direct3D11::ID3D11Texture2D,
        Media::MediaFoundation::{
            IMF2DBuffer, IMFAsyncCallback, IMFAsyncResult, IMFAttributes, IMFByteStream,
            IMFByteStream_Impl, IMFDXGIBuffer, IMFMediaBuffer, IMFMediaType, IMFSample,
            IMFSourceReader, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
            MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE,
            MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
            MF_MT_VIDEO_ROTATION, MF_MT_YUV_MATRIX, MF_PD_DURATION,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SOURCE_READER_ALL_STREAMS,
            MF_SOURCE_READER_D3D_MANAGER, MF_SOURCE_READER_FIRST_AUDIO_STREAM,
            MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
            MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
            MF_SOURCE_READERF_ERROR, MF_VERSION, MFAudioFormat_Float, MFBYTESTREAM_IS_READABLE,
            MFBYTESTREAM_IS_SEEKABLE, MFBYTESTREAM_SEEK_ORIGIN, MFCreateAsyncResult,
            MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromByteStream,
            MFCreateSourceReaderFromURL, MFInvokeCallback, MFMediaType_Audio, MFMediaType_Video,
            MFNominalRange_0_255, MFSTARTUP_NOSOCKET, MFShutdown, MFStartup, MFVideoArea,
            MFVideoFormat_NV12, MFVideoFormat_P010, MFVideoPrimaries_BT470_2_SysBG,
            MFVideoPrimaries_BT2020, MFVideoPrimaries_SMPTE170M, MFVideoTransFunc_2084,
            MFVideoTransFunc_HLG, MFVideoTransferMatrix_BT601, MFVideoTransferMatrix_BT2020_10,
            MFVideoTransferMatrix_BT2020_12, msoBegin, msoCurrent,
        },
        System::Com::{
            COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
        },
    },
    core::{BOOL, Error as WinError, GUID, HSTRING, IUnknown, Interface, Ref, Result as WinResult},
};
use windows_core::implement;

use crate::player_stream::HttpRangeReader;

const HNS_PER_SEC: u64 = 10_000_000;

pub(crate) struct MfSession {
    com_initialized: bool,
    mf_started: bool,
}

impl MfSession {
    pub(crate) fn enter() -> Result<Self, String> {
        // SAFETY: initializes COM MTA for the player worker thread.
        let com_hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = com_hr.is_ok();
        // SAFETY: paired with `MFShutdown` in `Drop`.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }
            .map_err(|err| format!("MFStartup failed: {err}"))?;
        Ok(Self {
            com_initialized,
            mf_started: true,
        })
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        if self.mf_started {
            // SAFETY: balances `MFStartup`.
            let _ = unsafe { MFShutdown() };
        }
        if self.com_initialized {
            // SAFETY: balances `CoInitializeEx` on this worker thread.
            unsafe { CoUninitialize() };
        }
    }
}

enum ByteSource {
    Memory(Arc<[u8]>),
    Http(Box<HttpRangeReader>),
}

struct ByteStreamState {
    source: ByteSource,
    pos: u64,
    closed: bool,
    pending_reads: BTreeMap<usize, u32>,
}

#[implement(IMFByteStream)]
struct CranposeByteStream {
    state: Mutex<ByteStreamState>,
}

impl CranposeByteStream {
    fn from_bytes(bytes: Arc<[u8]>) -> IMFByteStream {
        Self {
            state: Mutex::new(ByteStreamState {
                source: ByteSource::Memory(bytes),
                pos: 0,
                closed: false,
                pending_reads: BTreeMap::new(),
            }),
        }
        .into()
    }

    fn from_http(reader: HttpRangeReader) -> IMFByteStream {
        Self {
            state: Mutex::new(ByteStreamState {
                source: ByteSource::Http(Box::new(reader)),
                pos: 0,
                closed: false,
                pending_reads: BTreeMap::new(),
            }),
        }
        .into()
    }

    fn read_into(&self, dst: &mut [u8]) -> WinResult<u32> {
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if st.closed || dst.is_empty() {
            return Ok(0);
        }
        let pos = st.pos;
        let count = match &mut st.source {
            ByteSource::Memory(bytes) => {
                let offset = usize::try_from(pos).unwrap_or(bytes.len());
                if offset >= bytes.len() {
                    0
                } else {
                    let n = (bytes.len() - offset).min(dst.len());
                    dst[..n].copy_from_slice(&bytes[offset..offset + n]);
                    n
                }
            }
            ByteSource::Http(reader) => reader.read_at(pos, dst).unwrap_or(0),
        };
        st.pos = pos.saturating_add(count as u64);
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    fn total_length(source: &ByteSource) -> u64 {
        match source {
            ByteSource::Memory(bytes) => bytes.len() as u64,
            ByteSource::Http(reader) => reader.size().unwrap_or(0),
        }
    }
}

impl IMFByteStream_Impl for CranposeByteStream_Impl {
    fn GetCapabilities(&self) -> WinResult<u32> {
        Ok(MFBYTESTREAM_IS_READABLE | MFBYTESTREAM_IS_SEEKABLE)
    }

    fn GetLength(&self) -> WinResult<u64> {
        let st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(CranposeByteStream::total_length(&st.source))
    }

    fn SetLength(&self, _qwlength: u64) -> WinResult<()> {
        Err(WinError::from(E_NOTIMPL))
    }

    fn GetCurrentPosition(&self) -> WinResult<u64> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pos)
    }

    fn SetCurrentPosition(&self, qwposition: u64) -> WinResult<()> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pos = qwposition;
        Ok(())
    }

    fn IsEndOfStream(&self) -> WinResult<BOOL> {
        let st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let len = CranposeByteStream::total_length(&st.source);
        Ok(BOOL::from(len > 0 && st.pos >= len))
    }

    fn Read(&self, pb: *mut u8, cb: u32, pcbread: *mut u32) -> WinResult<()> {
        if pb.is_null() || pcbread.is_null() {
            return Err(WinError::from(E_POINTER));
        }
        // SAFETY: COM contract guarantees `pb` is writable for `cb` bytes and `pcbread` is valid.
        let dst = unsafe { std::slice::from_raw_parts_mut(pb, cb as usize) };
        let read = self.read_into(dst)?;
        unsafe { *pcbread = read };
        Ok(())
    }

    fn BeginRead(
        &self,
        pb: *mut u8,
        cb: u32,
        pcallback: Ref<IMFAsyncCallback>,
        punkstate: Ref<IUnknown>,
    ) -> WinResult<()> {
        if pb.is_null() {
            return Err(WinError::from(E_POINTER));
        }
        // SAFETY: COM contract guarantees `pb` is writable for `cb` bytes.
        let dst = unsafe { std::slice::from_raw_parts_mut(pb, cb as usize) };
        let read = self.read_into(dst)?;
        // SAFETY: creates an MF async result and dispatches the caller's callback immediately.
        let async_result =
            unsafe { MFCreateAsyncResult(None, pcallback.as_ref(), punkstate.as_ref()) }?;
        let key = async_result.as_raw() as usize;
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_reads
            .insert(key, read);
        unsafe { MFInvokeCallback(&async_result) }
    }

    fn EndRead(&self, presult: Ref<IMFAsyncResult>) -> WinResult<u32> {
        let result = presult.ok()?;
        let key = result.as_raw() as usize;
        Ok(self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending_reads
            .remove(&key)
            .unwrap_or(0))
    }

    fn Write(&self, _pb: *const u8, _cb: u32) -> WinResult<u32> {
        Err(WinError::from(E_NOTIMPL))
    }

    fn BeginWrite(
        &self,
        _pb: *const u8,
        _cb: u32,
        _pcallback: Ref<IMFAsyncCallback>,
        _punkstate: Ref<IUnknown>,
    ) -> WinResult<()> {
        Err(WinError::from(E_NOTIMPL))
    }

    fn EndWrite(&self, _presult: Ref<IMFAsyncResult>) -> WinResult<u32> {
        Err(WinError::from(E_NOTIMPL))
    }

    fn Seek(
        &self,
        seekorigin: MFBYTESTREAM_SEEK_ORIGIN,
        llseekoffset: i64,
        _dwseekflags: u32,
    ) -> WinResult<u64> {
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let base = if seekorigin == msoBegin {
            0_i128
        } else if seekorigin == msoCurrent {
            i128::from(st.pos)
        } else {
            return Err(WinError::from(E_INVALIDARG));
        };
        let target = base + i128::from(llseekoffset);
        if target < 0 {
            return Err(WinError::from(E_INVALIDARG));
        }
        let next = u64::try_from(target).unwrap_or(u64::MAX);
        st.pos = next;
        Ok(next)
    }

    fn Flush(&self) -> WinResult<()> {
        Ok(())
    }

    fn Close(&self) -> WinResult<()> {
        let mut st = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        st.closed = true;
        st.pending_reads.clear();
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct VideoOutputMeta {
    size: (u32, u32),
    crop: CropRect,
    format: DxgiPixelFormat,
    color: VideoColorInfo,
    rotation: VideoRotation,
}

#[derive(Clone, Copy)]
pub(crate) struct AudioOutputMeta {
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
}

pub(crate) struct OpenedMediaInfo {
    pub(crate) has_video: bool,
    pub(crate) video_size: VideoSize,
    pub(crate) audio: Option<AudioOutputMeta>,
    pub(crate) duration: Option<Duration>,
    pub(crate) tracks: Tracks,
}

pub(crate) enum ReadMediaSample {
    Video(VideoFrame),
    Audio {
        pts: Duration,
        samples: Vec<f32>,
        sample_rate: u32,
        channels: usize,
    },
    EndOfStream,
}

pub(crate) struct MfSource {
    reader: IMFSourceReader,
    exporter: Option<D3d11SurfaceExporter>,
    video_meta: Option<VideoOutputMeta>,
    audio_meta: Option<AudioOutputMeta>,
    video_stream_index: Option<u32>,
    audio_stream_index: Option<u32>,
    has_video: bool,
    has_audio: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamReadKind {
    VideoOnly,
    AudioOnly,
    Any,
}

impl MfSource {
    pub(crate) fn from_file(path: &Path) -> Result<(Self, OpenedMediaInfo), String> {
        let (exporter, attrs) = create_reader_attributes()?;
        let url = HSTRING::from(path.as_os_str());
        // SAFETY: `MFCreateSourceReaderFromURL` opens a local media file with hardware transforms.
        let reader = unsafe { MFCreateSourceReaderFromURL(&url, &attrs) }
            .map_err(|err| format!("MFCreateSourceReaderFromURL failed: {err}"))?;
        Self::configure(reader, exporter)
    }

    pub(crate) fn from_bytes(bytes: Arc<[u8]>) -> Result<(Self, OpenedMediaInfo), String> {
        let stream = CranposeByteStream::from_bytes(bytes);
        Self::from_byte_stream(&stream)
    }

    pub(crate) fn from_http(reader: HttpRangeReader) -> Result<(Self, OpenedMediaInfo), String> {
        let stream = CranposeByteStream::from_http(reader);
        Self::from_byte_stream(&stream)
    }

    fn from_byte_stream(stream: &IMFByteStream) -> Result<(Self, OpenedMediaInfo), String> {
        let (exporter, attrs) = create_reader_attributes()?;
        // SAFETY: `stream` implements `IMFByteStream` for in-memory bytes or HTTP range reads.
        let reader = unsafe { MFCreateSourceReaderFromByteStream(stream, &attrs) }
            .map_err(|err| format!("MFCreateSourceReaderFromByteStream failed: {err}"))?;
        Self::configure(reader, exporter)
    }

    fn configure(
        reader: IMFSourceReader,
        exporter: Option<D3d11SurfaceExporter>,
    ) -> Result<(Self, OpenedMediaInfo), String> {
        // SAFETY: deselect all streams, then select first video and audio streams explicitly.
        let _ = unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false) };

        let video_idx = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let audio_idx = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

        let mut has_video = false;
        let mut video_meta = None;
        if unsafe { reader.SetStreamSelection(video_idx, true) }.is_ok() {
            configure_video_output(&reader, video_idx);
            if let Ok(mt) = unsafe { reader.GetCurrentMediaType(video_idx) } {
                video_meta = Some(read_video_meta(&mt));
                has_video = true;
            }
        }

        let mut has_audio = false;
        let mut audio_meta = None;
        if unsafe { reader.SetStreamSelection(audio_idx, true) }.is_ok() {
            configure_audio_output(&reader, audio_idx);
            if let Ok(mt) = unsafe { reader.GetCurrentMediaType(audio_idx) } {
                audio_meta = Some(read_audio_meta(&mt));
                has_audio = true;
            }
        }

        if !has_video && !has_audio {
            return Err("media source contains neither a playable video nor audio stream".into());
        }

        let duration = read_duration(&reader);
        let tracks = build_tracks(video_meta, audio_meta);
        let video_size = video_meta.map_or(VideoSize::ZERO, |m| {
            let (w, h) = if m.rotation.swaps_axes() {
                (m.crop.height, m.crop.width)
            } else {
                (m.crop.width, m.crop.height)
            };
            VideoSize::new(w, h)
        });

        let info = OpenedMediaInfo {
            has_video,
            video_size,
            audio: audio_meta,
            duration,
            tracks,
        };
        Ok((
            Self {
                reader,
                exporter,
                video_meta,
                audio_meta,
                video_stream_index: None,
                audio_stream_index: None,
                has_video,
                has_audio,
            },
            info,
        ))
    }

    pub(crate) fn seek(&mut self, position: Duration) -> Result<(), String> {
        let hns = i64::try_from(position.as_nanos() / 100).unwrap_or(i64::MAX);
        let prop = PROPVARIANT::from(hns);
        // SAFETY: `GUID::zeroed()` selects 100-nanosecond Media Foundation time units.
        unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &prop) }
            .map_err(|err| format!("IMFSourceReader::SetCurrentPosition failed: {err}"))
    }

    pub(crate) fn read_next(&mut self, kind: StreamReadKind) -> Result<ReadMediaSample, String> {
        let want_video = matches!(kind, StreamReadKind::VideoOnly | StreamReadKind::Any);
        let want_audio = matches!(kind, StreamReadKind::AudioOnly | StreamReadKind::Any);
        let stream_selector = match (want_video && self.has_video, want_audio && self.has_audio) {
            (true, true) => MF_SOURCE_READER_ALL_STREAMS.0 as u32,
            (true, false) => MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
            (false, true) => MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32,
            (false, false) => return Ok(ReadMediaSample::EndOfStream),
        };
        loop {
            let mut actual_stream = 0u32;
            let mut flags = 0u32;
            let mut timestamp_hns = 0i64;
            let mut sample: Option<IMFSample> = None;
            // SAFETY: reads the next decoded video or audio sample synchronously on the worker thread.
            unsafe {
                self.reader.ReadSample(
                    stream_selector,
                    0,
                    Some(&mut actual_stream),
                    Some(&mut flags),
                    Some(&mut timestamp_hns),
                    Some(&mut sample),
                )
            }
            .map_err(|err| format!("IMFSourceReader::ReadSample failed: {err}"))?;

            if (flags & MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
                return Err("Media Foundation source reader reported a stream error".into());
            }
            if (flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0 {
                self.refresh_stream_type(actual_stream);
            }
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                return Ok(ReadMediaSample::EndOfStream);
            }
            let Some(sample) = sample else {
                continue;
            };
            let pts = hns_to_duration(timestamp_hns);
            if self.is_video_stream(actual_stream) {
                if let Some(frame) = self.export_video_sample(&sample, pts)? {
                    return Ok(ReadMediaSample::Video(frame));
                }
            } else if self.is_audio_stream(actual_stream) {
                let meta = self.audio_meta.unwrap_or(AudioOutputMeta {
                    sample_rate: 48_000,
                    channels: 2,
                });
                let samples = extract_audio_f32(&sample)?;
                if !samples.is_empty() {
                    return Ok(ReadMediaSample::Audio {
                        pts,
                        samples,
                        sample_rate: meta.sample_rate,
                        channels: meta.channels,
                    });
                }
            }
        }
    }

    fn refresh_stream_type(&mut self, stream_index: u32) {
        let Ok(mt) = (unsafe { self.reader.GetCurrentMediaType(stream_index) }) else {
            return;
        };
        let Ok(major) = (unsafe { mt.GetGUID(&MF_MT_MAJOR_TYPE) }) else {
            return;
        };
        if major == MFMediaType_Video {
            self.video_stream_index = Some(stream_index);
            self.video_meta = Some(read_video_meta(&mt));
        } else if major == MFMediaType_Audio {
            self.audio_stream_index = Some(stream_index);
            self.audio_meta = Some(read_audio_meta(&mt));
        }
    }

    fn is_video_stream(&mut self, stream_index: u32) -> bool {
        if self.video_stream_index == Some(stream_index) {
            return true;
        }
        if self.audio_stream_index == Some(stream_index) {
            return false;
        }
        self.refresh_stream_type(stream_index);
        self.video_stream_index == Some(stream_index)
    }

    fn is_audio_stream(&mut self, stream_index: u32) -> bool {
        if self.audio_stream_index == Some(stream_index) {
            return true;
        }
        if self.video_stream_index == Some(stream_index) {
            return false;
        }
        self.refresh_stream_type(stream_index);
        self.audio_stream_index == Some(stream_index)
    }

    fn export_video_sample(
        &mut self,
        sample: &IMFSample,
        pts: Duration,
    ) -> Result<Option<VideoFrame>, String> {
        let Some(exporter) = self.exporter.as_mut() else {
            return Ok(None);
        };
        let meta = self.video_meta.unwrap_or_else(|| VideoOutputMeta {
            size: (0, 0),
            crop: CropRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            format: DxgiPixelFormat::Nv12,
            color: VideoColorInfo::default(),
            rotation: VideoRotation::Degrees0,
        });
        // SAFETY: converts sample buffers into one contiguous or DXGI buffer.
        let buffer: IMFMediaBuffer = unsafe { sample.ConvertToContiguousBuffer() }
            .or_else(|_| unsafe { sample.GetBufferByIndex(0) })
            .map_err(|err| format!("IMFSample video buffer: {err}"))?;
        let desc = D3d11FrameDescriptor {
            info: FrameInfo {
                timestamp: pts,
                metadata: 0,
                decode_time: Duration::ZERO,
            },
            crop: meta.crop,
            color: meta.color,
            rotation: meta.rotation,
        };

        if let Some(frame) = try_export_dxgi_buffer(exporter, &buffer, desc)? {
            return Ok(Some(frame));
        }
        export_cpu_video_buffer(exporter, &buffer, meta, desc)
    }
}

fn try_export_dxgi_buffer(
    exporter: &mut D3d11SurfaceExporter,
    buffer: &IMFMediaBuffer,
    desc: D3d11FrameDescriptor,
) -> Result<Option<VideoFrame>, String> {
    let Ok(dxgi_buf) = buffer.cast::<IMFDXGIBuffer>() else {
        return Ok(None);
    };
    let mut tex_ptr: *mut c_void = ptr::null_mut();
    if unsafe { dxgi_buf.GetResource(&ID3D11Texture2D::IID, &mut tex_ptr) }.is_err()
        || tex_ptr.is_null()
    {
        return Ok(None);
    }
    // SAFETY: `GetResource` returned an owned COM reference to `ID3D11Texture2D`.
    let texture = unsafe { ID3D11Texture2D::from_raw(tex_ptr) };
    let subresource = unsafe { dxgi_buf.GetSubresourceIndex() }.unwrap_or(0);
    let frame = unsafe { exporter.export_gpu_texture_ptr(texture.as_raw(), subresource, desc) }
        .map_err(|err| format!("D3D11 zero-copy export failed: {err}"))?;
    Ok(Some(frame))
}

fn export_cpu_video_buffer(
    exporter: &mut D3d11SurfaceExporter,
    buffer: &IMFMediaBuffer,
    meta: VideoOutputMeta,
    desc: D3d11FrameDescriptor,
) -> Result<Option<VideoFrame>, String> {
    if meta.size.0 == 0 || meta.size.1 == 0 {
        return Ok(None);
    }
    if let Ok(buf2d) = buffer.cast::<IMF2DBuffer>() {
        let mut scanline0: *mut u8 = ptr::null_mut();
        let mut pitch: i32 = 0;
        if unsafe { buf2d.Lock2D(&mut scanline0, &mut pitch) }.is_ok()
            && !scanline0.is_null()
            && pitch > 0
        {
            let exported = unsafe {
                exporter.export_cpu_buffer(scanline0, pitch as u32, meta.size, meta.format, desc)
            };
            let _ = unsafe { buf2d.Unlock2D() };
            return exported
                .map(Some)
                .map_err(|err| format!("D3D11 CPU upload failed: {err}"));
        }
    }
    let mut data_ptr: *mut u8 = ptr::null_mut();
    let mut cur_len: u32 = 0;
    unsafe { buffer.Lock(&mut data_ptr, None, Some(&mut cur_len)) }
        .map_err(|err| format!("IMFMediaBuffer::Lock failed: {err}"))?;
    let stride = match meta.format {
        DxgiPixelFormat::Nv12 => meta.size.0,
        DxgiPixelFormat::P010 => meta.size.0 * 2,
    };
    let required = stride.saturating_mul(meta.size.1).saturating_mul(3) / 2;
    let exported = if !data_ptr.is_null() && cur_len >= required {
        unsafe { exporter.export_cpu_buffer(data_ptr, stride, meta.size, meta.format, desc) }
            .map(Some)
            .map_err(|err| format!("D3D11 CPU upload failed: {err}"))
    } else {
        Ok(None)
    };
    let _ = unsafe { buffer.Unlock() };
    exported
}

fn create_reader_attributes() -> Result<(Option<D3d11SurfaceExporter>, IMFAttributes), String> {
    let mut attrs: Option<IMFAttributes> = None;
    // SAFETY: allocates a Media Foundation attribute store for `IMFSourceReader`.
    unsafe { MFCreateAttributes(&mut attrs, 4) }
        .map_err(|err| format!("MFCreateAttributes failed: {err}"))?;
    let attrs = attrs.ok_or_else(|| "MFCreateAttributes returned null".to_owned())?;
    let _ = unsafe { attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1) };

    let exporter = D3d11SurfaceExporter::new().ok();
    if let Some(ref exp) = exporter {
        let raw_mgr = exp.dxgi_device_manager_ptr();
        // SAFETY: `raw_mgr` is a live `IMFDXGIDeviceManager*` borrowed for `SetUnknown`.
        if let Some(unk) = unsafe { IUnknown::from_raw_borrowed(&raw_mgr) } {
            let _ = unsafe { attrs.SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, unk) };
        }
    }
    Ok((exporter, attrs))
}

fn configure_video_output(reader: &IMFSourceReader, stream_index: u32) {
    let prefer_p010 = unsafe { reader.GetNativeMediaType(stream_index, 0) }
        .ok()
        .is_some_and(|native| is_hdr_transfer(&native));
    let subtypes = if prefer_p010 {
        [MFVideoFormat_P010, MFVideoFormat_NV12]
    } else {
        [MFVideoFormat_NV12, MFVideoFormat_P010]
    };
    for subtype in subtypes {
        let Ok(mt) = (unsafe { MFCreateMediaType() }) else {
            return;
        };
        if unsafe { mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }.is_ok()
            && unsafe { mt.SetGUID(&MF_MT_SUBTYPE, &subtype) }.is_ok()
            && unsafe { reader.SetCurrentMediaType(stream_index, None, &mt) }.is_ok()
        {
            return;
        }
    }
}

fn configure_audio_output(reader: &IMFSourceReader, stream_index: u32) {
    let Ok(mt) = (unsafe { MFCreateMediaType() }) else {
        return;
    };
    if unsafe { mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio) }.is_ok()
        && unsafe { mt.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float) }.is_ok()
    {
        let _ = unsafe { reader.SetCurrentMediaType(stream_index, None, &mt) };
    }
}

fn is_hdr_transfer(mt: &IMFMediaType) -> bool {
    let tf = unsafe { mt.GetUINT32(&MF_MT_TRANSFER_FUNCTION) }.unwrap_or(0) as i32;
    tf == MFVideoTransFunc_2084.0 || tf == MFVideoTransFunc_HLG.0
}

fn read_video_meta(mt: &IMFMediaType) -> VideoOutputMeta {
    let packed = unsafe { mt.GetUINT64(&MF_MT_FRAME_SIZE) }.unwrap_or(0);
    let width = (packed >> 32) as u32;
    let height = packed as u32;
    let crop = read_crop_rect(mt, width, height);
    let subtype = unsafe { mt.GetGUID(&MF_MT_SUBTYPE) }.unwrap_or(MFVideoFormat_NV12);
    let format = if subtype == MFVideoFormat_P010 {
        DxgiPixelFormat::P010
    } else {
        DxgiPixelFormat::Nv12
    };
    let color = read_color_info(mt);
    let rotation = match unsafe { mt.GetUINT32(&MF_MT_VIDEO_ROTATION) }.unwrap_or(0) {
        90 => VideoRotation::Degrees90,
        180 => VideoRotation::Degrees180,
        270 => VideoRotation::Degrees270,
        _ => VideoRotation::Degrees0,
    };
    VideoOutputMeta {
        size: (width, height),
        crop,
        format,
        color,
        rotation,
    }
}

fn read_crop_rect(mt: &IMFMediaType, width: u32, height: u32) -> CropRect {
    let mut area = MFVideoArea::default();
    let mut copied = 0u32;
    let dst = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut area as *mut MFVideoArea).cast::<u8>(),
            size_of::<MFVideoArea>(),
        )
    };
    if unsafe { mt.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, dst, Some(&mut copied)) }.is_ok()
        && copied as usize == size_of::<MFVideoArea>()
        && area.Area.cx > 0
        && area.Area.cy > 0
    {
        return CropRect {
            x: u32::try_from(area.OffsetX.value).unwrap_or(0),
            y: u32::try_from(area.OffsetY.value).unwrap_or(0),
            width: u32::try_from(area.Area.cx).unwrap_or(width),
            height: u32::try_from(area.Area.cy).unwrap_or(height),
        };
    }
    CropRect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

fn read_color_info(mt: &IMFMediaType) -> VideoColorInfo {
    let primaries_raw = unsafe { mt.GetUINT32(&MF_MT_VIDEO_PRIMARIES) }.unwrap_or(0) as i32;
    let matrix_raw = unsafe { mt.GetUINT32(&MF_MT_YUV_MATRIX) }.unwrap_or(0) as i32;
    let primaries = if primaries_raw == MFVideoPrimaries_BT2020.0
        || matrix_raw == MFVideoTransferMatrix_BT2020_10.0
        || matrix_raw == MFVideoTransferMatrix_BT2020_12.0
    {
        ColorPrimaries::Bt2020
    } else if primaries_raw == MFVideoPrimaries_SMPTE170M.0
        || primaries_raw == MFVideoPrimaries_BT470_2_SysBG.0
        || matrix_raw == MFVideoTransferMatrix_BT601.0
    {
        ColorPrimaries::Bt601
    } else {
        ColorPrimaries::Bt709
    };

    let range_raw = unsafe { mt.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE) }.unwrap_or(0) as i32;
    let range = if range_raw == MFNominalRange_0_255.0 {
        ColorRange::Full
    } else {
        ColorRange::Limited
    };

    let transfer_raw = unsafe { mt.GetUINT32(&MF_MT_TRANSFER_FUNCTION) }.unwrap_or(0) as i32;
    let transfer = if transfer_raw == MFVideoTransFunc_2084.0 {
        ColorTransfer::Pq
    } else if transfer_raw == MFVideoTransFunc_HLG.0 {
        ColorTransfer::Hlg
    } else {
        ColorTransfer::Sdr
    };

    VideoColorInfo {
        primaries,
        range,
        transfer,
    }
}

fn read_audio_meta(mt: &IMFMediaType) -> AudioOutputMeta {
    let sample_rate = unsafe { mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }
        .unwrap_or(48_000)
        .max(8_000);
    let channels = usize::try_from(unsafe { mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }.unwrap_or(2))
        .unwrap_or(2)
        .max(1);
    AudioOutputMeta {
        sample_rate,
        channels,
    }
}

fn read_duration(reader: &IMFSourceReader) -> Option<Duration> {
    let prop = unsafe {
        reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
    }
    .ok()?;
    let hns = i64::try_from(&prop).ok().filter(|&v| v > 0).or_else(|| {
        u64::try_from(&prop)
            .ok()
            .and_then(|v| i64::try_from(v).ok())
    })?;
    (hns > 0).then(|| hns_to_duration(hns))
}

fn hns_to_duration(hns: i64) -> Duration {
    if hns <= 0 {
        return Duration::ZERO;
    }
    let u = hns as u64;
    let secs = u / HNS_PER_SEC;
    let rem_ns = (u % HNS_PER_SEC) * 100;
    Duration::new(secs, rem_ns as u32)
}

fn extract_audio_f32(sample: &IMFSample) -> Result<Vec<f32>, String> {
    let buffer: IMFMediaBuffer = unsafe { sample.ConvertToContiguousBuffer() }
        .map_err(|err| format!("IMFSample audio buffer: {err}"))?;
    let mut ptr: *mut u8 = ptr::null_mut();
    let mut len: u32 = 0;
    unsafe { buffer.Lock(&mut ptr, None, Some(&mut len)) }
        .map_err(|err| format!("audio IMFMediaBuffer::Lock: {err}"))?;
    let mut out = Vec::new();
    if !ptr.is_null() && len >= 4 {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        let (chunks, _) = bytes.as_chunks::<4>();
        out.reserve(chunks.len());
        for chunk in chunks {
            out.push(f32::from_le_bytes(*chunk));
        }
    }
    let _ = unsafe { buffer.Unlock() };
    Ok(out)
}

fn build_tracks(video: Option<VideoOutputMeta>, audio: Option<AudioOutputMeta>) -> Tracks {
    let mut groups = Vec::new();
    if let Some(v) = video {
        groups.push(TrackGroup {
            id: "video-0".into(),
            track_type: TrackType::Video,
            adaptive_supported: false,
            tracks: vec![TrackEntry {
                format: TrackFormat {
                    id: "video-0".into(),
                    mime_type: Some("video/mp4".into()),
                    codecs: None,
                    label: None,
                    language: None,
                    bitrate: None,
                    width: Some(v.crop.width),
                    height: Some(v.crop.height),
                    frame_rate: None,
                    channel_count: None,
                    sample_rate: None,
                },
                selected: true,
                supported: true,
            }],
        });
    }
    if let Some(a) = audio {
        groups.push(TrackGroup {
            id: "audio-0".into(),
            track_type: TrackType::Audio,
            adaptive_supported: false,
            tracks: vec![TrackEntry {
                format: TrackFormat {
                    id: "audio-0".into(),
                    mime_type: Some("audio/raw".into()),
                    codecs: None,
                    label: None,
                    language: None,
                    bitrate: None,
                    width: None,
                    height: None,
                    frame_rate: None,
                    channel_count: u16::try_from(a.channels).ok(),
                    sample_rate: Some(a.sample_rate),
                },
                selected: true,
                supported: true,
            }],
        });
    }
    Tracks { groups }
}
