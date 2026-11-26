//! Native zero-copy Windows [`PlayerBackend`] built on Media Foundation `IMFSourceReader` +
//! D3D11/DXGI shared surfaces into D3D12 `D3d12Converter` + `cranpose-audio` WASAPI output
//! anchored to `MasterClock` (<= 40 ms lip-sync).

use std::{sync::Arc, time::Duration};

use cranpose_services::{
    MediaItem, PlayerCapabilities, PlayerError, TrackSelectionParameters, VideoContainer,
};

use super::player_source::{MfSession, MfSource, OpenedMediaInfo, ReadMediaSample, StreamReadKind};
use crate::{
    player_stream::{
        AdaptiveStream, FetchCancel, HttpRangeReader, detect_manifest_kind, is_http_uri,
        local_path_from_uri,
    },
    player_worker::{
        AUDIO_TARGET_BUFFER, AudioPresence, FRAME_QUEUE_DEPTH, NativePlaybackEngine, OpenedMedia,
        StepContext, open_adaptive_first_segment, probe_decoder_capabilities,
        register_native_player,
    },
};

pub(crate) fn register() {
    register_native_player::<WindowsPlaybackEngine>();
}

#[derive(Default)]
struct WindowsPlaybackEngine {
    _mf_session: Option<MfSession>,
}

struct WindowsMedia {
    source: MfSource,
    has_video: bool,
    has_audio: bool,
}

impl NativePlaybackEngine for WindowsPlaybackEngine {
    type Media = WindowsMedia;

    fn thread_name() -> &'static str {
        "cranpose-windows-player"
    }

    fn capabilities() -> PlayerCapabilities {
        probe_decoder_capabilities(vec![
            VideoContainer::Mp4,
            VideoContainer::FragmentedMp4,
            VideoContainer::Mkv,
            VideoContainer::Webm,
            VideoContainer::Hls,
            VideoContainer::Dash,
        ])
    }

    fn on_thread_start(&mut self) -> Result<(), PlayerError> {
        let session = MfSession::enter().map_err(PlayerError::Decoder)?;
        self._mf_session = Some(session);
        Ok(())
    }

    fn open(
        &mut self,
        item: &MediaItem,
        initial_target: Duration,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedMedia<Self::Media>, PlayerError> {
        let (mut source, info, adaptive, segment_base_pts) = open_source(item, selection, cancel)?;
        if adaptive.is_none() && initial_target > Duration::ZERO {
            drop(source.seek(initial_target));
        }
        let has_audio = info.audio.is_some();
        let audio = if has_audio {
            AudioPresence::Present
        } else {
            AudioPresence::Absent
        };
        Ok(OpenedMedia::from_adaptive(
            WindowsMedia {
                source,
                has_video: info.has_video,
                has_audio,
            },
            adaptive,
            segment_base_pts,
            audio,
            info.has_video.then_some(info.video_size),
            info.tracks,
            info.duration,
        ))
    }

    fn step(
        &mut self,
        media: &mut Self::Media,
        ctx: &mut StepContext<'_>,
    ) -> Result<bool, PlayerError> {
        for _ in 0..4 {
            let want_v = media.has_video && ctx.pending_video.len() < FRAME_QUEUE_DEPTH;
            let want_a = media.has_audio && ctx.audio.buffered_duration() < AUDIO_TARGET_BUFFER;
            let read_kind = match (want_v, want_a) {
                (true, true) => StreamReadKind::Any,
                (true, false) => StreamReadKind::VideoOnly,
                (false, true) => StreamReadKind::AudioOnly,
                (false, false) => break,
            };
            match media
                .source
                .read_next(read_kind)
                .map_err(PlayerError::Decoder)?
            {
                ReadMediaSample::Video(frame) => ctx.pending_video.push_back(frame),
                ReadMediaSample::Audio {
                    pts,
                    samples,
                    sample_rate,
                    channels,
                } => {
                    let abs_pts = ctx.segment_base_pts.saturating_add(pts);
                    let drop_pre_seek = ctx
                        .seek_target
                        .is_some_and(|t| abs_pts.saturating_add(Duration::from_millis(25)) < t);
                    if !drop_pre_seek {
                        ctx.audio.push_pcm_f32(&samples, sample_rate, channels);
                    }
                }
                ReadMediaSample::EndOfStream => return Ok(true),
            }
        }
        Ok(false)
    }

    fn replace_segment(
        &mut self,
        media: &mut Self::Media,
        bytes: Arc<[u8]>,
        intra_seek: Duration,
    ) -> bool {
        let Ok((mut src, _)) = MfSource::from_bytes(bytes) else {
            return false;
        };
        if intra_seek > Duration::ZERO {
            drop(src.seek(intra_seek));
        }
        media.source = src;
        true
    }

    fn seek_direct(&mut self, media: &mut Self::Media, absolute_pos: Duration) {
        drop(media.source.seek(absolute_pos));
    }
}

fn open_source(
    item: &MediaItem,
    selection: &TrackSelectionParameters,
    cancel: &FetchCancel,
) -> Result<(MfSource, OpenedMediaInfo, Option<AdaptiveStream>, Duration), PlayerError> {
    if let Some(bytes) = &item.bytes {
        let (src, info) = MfSource::from_bytes(Arc::clone(bytes)).map_err(PlayerError::Source)?;
        return Ok((src, info, None, Duration::ZERO));
    }
    if let Some(path) = local_path_from_uri(&item.uri) {
        let (src, info) = MfSource::from_file(&path).map_err(PlayerError::Source)?;
        return Ok((src, info, None, Duration::ZERO));
    }
    if is_http_uri(&item.uri) {
        if let Some(kind) = detect_manifest_kind(item) {
            let (adaptive, bytes, start_pts) =
                open_adaptive_first_segment(item, kind, selection, cancel.clone())?;
            let (src, info) = MfSource::from_bytes(bytes).map_err(PlayerError::Source)?;
            return Ok((src, info, Some(adaptive), start_pts));
        }
        let reader = HttpRangeReader::open(&item.uri, item.headers.clone(), cancel.clone())
            .map_err(PlayerError::Network)?;
        let (src, info) = MfSource::from_http(reader).map_err(PlayerError::Network)?;
        return Ok((src, info, None, Duration::ZERO));
    }
    Err(PlayerError::Unsupported(format!(
        "unsupported Windows media URI: {}",
        item.uri
    )))
}
