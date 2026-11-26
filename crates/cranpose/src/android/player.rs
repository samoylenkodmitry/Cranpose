// Native zero-copy Android `PlayerBackend` built on NDK `AMediaExtractor` + `VideoDecoder`
// (`AMediaCodec` -> `AImageReader` -> `AHardwareBuffer` -> Vulkan `YcbcrConverter`) + audio
// `AMediaCodec` -> `cranpose-audio` AAudio anchored to `MasterClock` (<= 40 ms lip-sync).

use std::{fs::File, sync::Arc, time::Duration};

use cranpose_services::{
    AccessUnit, DecoderError, MediaItem, MediaMetadata, PlayerCapabilities, PlayerError,
    TrackSelectionParameters, VideoCodec, VideoContainer, VideoDecoder,
};

use super::{
    file_picker,
    media_session::{self, SESSION_PAUSED, SESSION_PLAYING, SESSION_STOPPED},
    player_extractor::{
        AudioCodecDecoder, AudioTrackInfo, DecodedAudio, MediaExtractor, VideoTrackInfo,
        ndk_has_decoder,
    },
};
use crate::{
    player_demux_engine::{
        DemuxAudioDecoder, FeedUnit, OpenedDemux, PacketDemuxer, PcmAudioOutput,
        SessionPlaybackEvent, open_adaptive_first_segment, probe_decoder_capabilities,
        register_demuxed_player,
    },
    player_stream::{
        AdaptiveStream, FetchCancel, HttpRangeReader, detect_manifest_kind, is_http_uri,
        local_path_from_uri,
    },
};

pub(crate) fn register() {
    register_demuxed_player::<AndroidDemuxer>();
}

impl DemuxAudioDecoder for AudioCodecDecoder {
    fn drain_pcm(&mut self, audio: &mut PcmAudioOutput) {
        while let Some(pcm) = self.dequeue_output() {
            match pcm {
                DecodedAudio::Pcm16 {
                    samples,
                    sample_rate,
                    channels,
                } => audio.push_pcm_i16(&samples, sample_rate, channels),
                DecodedAudio::PcmFloat {
                    samples,
                    sample_rate,
                    channels,
                } => audio.push_pcm_f32(&samples, sample_rate, channels),
            }
        }
    }

    fn flush(&mut self) {
        Self::flush(self);
    }

    fn send_eos(&mut self) {
        let _: bool = self.queue_eos();
    }
}

struct AndroidDemuxer {
    extractor: MediaExtractor,
    video_info: Option<VideoTrackInfo>,
    need_csd_prepend: bool,
    audio_tracks: Vec<AudioTrackInfo>,
    active_audio_idx: Option<usize>,
    demux_buf: Vec<u8>,
    packet_buf: Vec<u8>,
}

impl PacketDemuxer for AndroidDemuxer {
    type Audio = AudioCodecDecoder;

    fn thread_name() -> &'static str {
        "cranpose-android-player"
    }

    fn decoder_label() -> &'static str {
        "Android hardware"
    }

    fn capabilities() -> PlayerCapabilities {
        let mut caps = probe_decoder_capabilities(vec![
            VideoContainer::Mp4,
            VideoContainer::FragmentedMp4,
            VideoContainer::Mkv,
            VideoContainer::Webm,
            VideoContainer::Hls,
            VideoContainer::Dash,
        ]);
        for codec in [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Vp9,
            VideoCodec::Av1,
        ] {
            if !caps.supported_video_codecs.contains(&codec) && ndk_has_decoder(codec.mime()) {
                caps.supported_video_codecs.push(codec);
                if matches!(codec, VideoCodec::Hevc | VideoCodec::Vp9 | VideoCodec::Av1) {
                    caps.hdr_10bit = true;
                }
            }
        }
        if caps.supported_video_codecs.is_empty() {
            caps.supported_video_codecs.push(VideoCodec::H264);
        }
        caps.zero_copy_gpu = true;
        caps
    }

    fn open(
        item: &MediaItem,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedDemux<Self>, PlayerError> {
        let (extractor, adaptive, segment_base_pts) = open_extractor(item, selection, cancel)?;
        let extracted = extractor.inspect_tracks();
        let video_spec = extracted.video.as_ref().map(|v| v.spec);
        let audio_languages = extracted
            .audio_tracks
            .iter()
            .map(|t| t.language.clone())
            .collect();
        Ok(OpenedDemux {
            demux: Self {
                extractor,
                video_info: extracted.video,
                need_csd_prepend: true,
                audio_tracks: extracted.audio_tracks,
                active_audio_idx: None,
                demux_buf: Vec::with_capacity(64 * 1024),
                packet_buf: Vec::with_capacity(64 * 1024),
            },
            adaptive,
            segment_base_pts,
            video_spec,
            audio_languages,
            tracks: extracted.tracks,
            duration: extracted.duration,
        })
    }

    fn finalize_open(&mut self, initial_target: Duration) {
        if let Some(v) = &self.video_info {
            drop(self.extractor.select_track(v.track_index));
        }
        if initial_target > Duration::ZERO {
            drop(self.extractor.seek_to(initial_target));
        }
    }

    fn feed_next_unit(&mut self, unit: &mut FeedUnit<'_, Self::Audio>) -> Option<bool> {
        let sample = self.extractor.peek_sample(&mut self.demux_buf)?;
        let pts = unit.segment_base_pts.saturating_add(sample.pts);
        if self
            .video_info
            .as_ref()
            .is_some_and(|v| v.track_index == sample.track_index)
        {
            return Some(self.feed_video_sample(
                unit.video.as_deref_mut(),
                sample.size,
                pts,
                sample.is_sync,
            ));
        }
        if self
            .active_audio_idx
            .and_then(|idx| self.audio_tracks.get(idx))
            .is_some_and(|a| a.track_index == sample.track_index)
        {
            return Some(self.feed_audio_sample(
                unit.audio.as_deref_mut(),
                unit.seek_target,
                sample.size,
                pts,
            ));
        }
        self.extractor.advance();
        Some(true)
    }

    fn replace_segment(&mut self, bytes: Arc<[u8]>, intra_seek: Duration) -> bool {
        let Ok(mut next_ext) = MediaExtractor::from_bytes(bytes) else {
            return false;
        };
        reselect_tracks(
            &mut next_ext,
            self.video_info.as_ref(),
            &self.audio_tracks,
            self.active_audio_idx,
        );
        if intra_seek > Duration::ZERO {
            drop(next_ext.seek_to(intra_seek));
        }
        self.extractor = next_ext;
        self.need_csd_prepend = true;
        true
    }

    fn seek_direct(&mut self, absolute_pos: Duration) {
        self.need_csd_prepend = true;
        drop(self.extractor.seek_to(absolute_pos));
    }

    fn switch_audio(&mut self, prev: Option<usize>, next: Option<usize>) -> Option<Self::Audio> {
        if let Some(track) = prev.and_then(|idx| self.audio_tracks.get(idx)) {
            self.extractor.unselect_track(track.track_index);
        }
        self.active_audio_idx = next;
        next.and_then(|idx| self.audio_tracks.get(idx))
            .and_then(|track| {
                drop(self.extractor.select_track(track.track_index));
                self.extractor.open_audio_decoder(track.track_index).ok()
            })
    }

    fn buffered_position(&self, relative_pos: Duration) -> Duration {
        self.extractor
            .cached_duration()
            .map_or(relative_pos, |cached| relative_pos.saturating_add(cached))
    }

    fn request_primary_focus() -> bool {
        media_session::request_focus()
    }

    fn abandon_primary_focus() {
        media_session::abandon_focus();
    }

    fn notify_os_metadata(metadata: &MediaMetadata) {
        media_session::set_metadata(metadata);
    }

    fn notify_os_session(
        event: SessionPlaybackEvent,
        position: Duration,
        duration: Option<Duration>,
        speed: f32,
    ) {
        let code = match event {
            SessionPlaybackEvent::Playing => SESSION_PLAYING,
            SessionPlaybackEvent::Paused => SESSION_PAUSED,
            SessionPlaybackEvent::Stopped => SESSION_STOPPED,
        };
        media_session::update_session(code, position, duration, speed);
    }
}

impl AndroidDemuxer {
    fn feed_video_sample(
        &mut self,
        video_decoder: Option<&mut VideoDecoder>,
        size: usize,
        pts: Duration,
        is_sync: bool,
    ) -> bool {
        let payload = &self.demux_buf[..size];
        if let (Some(video_info), Some(decoder)) = (&self.video_info, video_decoder) {
            let needs_csd = matches!(video_info.spec.codec, VideoCodec::H264 | VideoCodec::Hevc)
                && !video_info.csd.is_empty()
                && (self.need_csd_prepend || is_sync);
            let unit_bytes = if needs_csd {
                self.packet_buf.clear();
                self.packet_buf.extend_from_slice(&video_info.csd);
                self.packet_buf.extend_from_slice(payload);
                self.packet_buf.as_slice()
            } else {
                payload
            };
            match decoder.submit(AccessUnit {
                data: unit_bytes,
                timestamp: pts,
                metadata: 0,
            }) {
                Ok(()) => {}
                Err(DecoderError::Full) => return false,
                Err(err) => log::warn!("android player: video submit error: {err}"),
            }
            if needs_csd {
                self.need_csd_prepend = false;
            }
        }
        self.extractor.advance();
        true
    }

    fn feed_audio_sample(
        &mut self,
        audio_decoder: Option<&mut AudioCodecDecoder>,
        seek_target: Option<Duration>,
        size: usize,
        pts: Duration,
    ) -> bool {
        if seek_target.is_some_and(|target| pts.saturating_add(Duration::from_millis(25)) < target)
        {
            self.extractor.advance();
            return true;
        }
        let payload = &self.demux_buf[..size];
        if let Some(audio_dec) = audio_decoder
            && !audio_dec.queue_input(payload, pts)
        {
            return false;
        }
        self.extractor.advance();
        true
    }
}

fn open_extractor(
    item: &MediaItem,
    selection: &TrackSelectionParameters,
    cancel: &FetchCancel,
) -> Result<(MediaExtractor, Option<AdaptiveStream>, Duration), PlayerError> {
    if let Some(bytes) = &item.bytes {
        let ext = MediaExtractor::from_bytes(Arc::clone(bytes)).map_err(PlayerError::Source)?;
        return Ok((ext, None, Duration::ZERO));
    }
    if item.uri.starts_with("content://") {
        let file = file_picker::open_content_uri(&item.uri)
            .map_err(|err| PlayerError::Source(format!("open content URI: {err}")))?;
        let ext = MediaExtractor::from_file(file).map_err(PlayerError::Source)?;
        return Ok((ext, None, Duration::ZERO));
    }
    if let Some(path) = local_path_from_uri(&item.uri) {
        let file =
            File::open(&path).map_err(|err| PlayerError::Source(format!("{}: {err}", item.uri)))?;
        let ext = MediaExtractor::from_file(file).map_err(PlayerError::Source)?;
        return Ok((ext, None, Duration::ZERO));
    }
    if is_http_uri(&item.uri) {
        if let Some(kind) = detect_manifest_kind(item) {
            let (adaptive, bytes, start_pts) =
                open_adaptive_first_segment(item, kind, selection, cancel.clone())?;
            let ext = MediaExtractor::from_bytes(bytes).map_err(PlayerError::Source)?;
            return Ok((ext, Some(adaptive), start_pts));
        }
        let reader = HttpRangeReader::open(&item.uri, item.headers.clone(), cancel.clone())
            .map_err(PlayerError::Network)?;
        let ext = MediaExtractor::from_http(reader)
            .or_else(|_| MediaExtractor::from_uri(&item.uri))
            .map_err(PlayerError::Network)?;
        return Ok((ext, None, Duration::ZERO));
    }
    let ext = MediaExtractor::from_uri(&item.uri).map_err(PlayerError::Source)?;
    Ok((ext, None, Duration::ZERO))
}

fn reselect_tracks(
    extractor: &mut MediaExtractor,
    video: Option<&VideoTrackInfo>,
    audio_tracks: &[AudioTrackInfo],
    active_audio: Option<usize>,
) {
    if let Some(v) = video {
        drop(extractor.select_track(v.track_index));
    }
    if let Some(idx) = active_audio
        && let Some(a) = audio_tracks.get(idx)
    {
        drop(extractor.select_track(a.track_index));
    }
}
