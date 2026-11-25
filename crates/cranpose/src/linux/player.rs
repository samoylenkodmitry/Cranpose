// Native zero-copy Linux `PlayerBackend` built on runtime-loaded `libavformat`/`libavcodec` +
// Phase 1 VA-API DMABUF `VideoDecoder` (with software fallback) + `cranpose-audio` cpal output
// anchored to `MasterClock` (<= 40 ms lip-sync).

use std::{sync::Arc, time::Duration};

use cranpose_services::{
    AccessUnit, DecoderError, MediaItem, PlayerCapabilities, PlayerError, TrackSelectionParameters,
    VideoContainer,
};

use super::player_demux::{
    AudioDecoder, AudioTrackInfo, Demuxer, VideoTrackInfo, avformat_available,
};
use crate::{
    player_demux_engine::{
        DemuxAudioDecoder, FeedUnit, OpenedDemux, PacketDemuxer, PcmAudioOutput,
        open_adaptive_first_segment, probe_decoder_capabilities, register_demuxed_player,
    },
    player_stream::{
        AdaptiveStream, FetchCancel, HttpRangeReader, detect_manifest_kind, is_http_uri,
        local_path_from_uri,
    },
};

pub(crate) fn register() {
    register_demuxed_player::<LinuxDemuxer>();
}

impl DemuxAudioDecoder for AudioDecoder {
    fn drain_pcm(&mut self, audio: &mut PcmAudioOutput) {
        while let Some(pcm) = self.receive_pcm() {
            audio.push_pcm_f32(&pcm.samples, pcm.sample_rate, pcm.channels);
        }
    }

    fn flush(&mut self) {
        Self::flush(self);
    }

    fn send_eos(&mut self) {
        Self::send_eos(self);
    }
}

struct LinuxDemuxer {
    demuxer: Demuxer,
    video_info: Option<VideoTrackInfo>,
    audio_tracks: Vec<AudioTrackInfo>,
    active_audio_idx: Option<usize>,
}

impl PacketDemuxer for LinuxDemuxer {
    type Audio = AudioDecoder;

    fn thread_name() -> &'static str {
        "cranpose-linux-player"
    }

    fn decoder_label() -> &'static str {
        "Linux"
    }

    fn capabilities() -> PlayerCapabilities {
        let containers = if avformat_available() {
            vec![
                VideoContainer::Mp4,
                VideoContainer::FragmentedMp4,
                VideoContainer::Mkv,
                VideoContainer::Webm,
                VideoContainer::Hls,
                VideoContainer::Dash,
            ]
        } else {
            Vec::new()
        };
        probe_decoder_capabilities(containers)
    }

    fn open(
        item: &MediaItem,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedDemux<Self>, PlayerError> {
        let (mut demuxer, adaptive, segment_base_pts) = open_demuxer(item, selection, cancel)?;
        let extracted = demuxer.inspect_tracks();
        let video_spec = extracted.video.as_ref().map(|v| v.spec);
        let audio_languages = extracted
            .audio_tracks
            .iter()
            .map(|t| t.language.clone())
            .collect();
        Ok(OpenedDemux {
            demux: Self {
                demuxer,
                video_info: extracted.video,
                audio_tracks: extracted.audio_tracks,
                active_audio_idx: None,
            },
            adaptive,
            segment_base_pts,
            video_spec,
            audio_languages,
            tracks: extracted.tracks,
            duration: extracted.duration,
        })
    }

    fn feed_next_unit(&mut self, unit: &mut FeedUnit<'_, Self::Audio>) -> Option<bool> {
        let packet = self.demuxer.peek_packet()?;
        let stream_index = packet.stream_index;
        let pts = unit.segment_base_pts.saturating_add(packet.pts);
        let is_video = self
            .video_info
            .as_ref()
            .is_some_and(|v| v.stream_index == stream_index);
        let is_audio = self
            .active_audio_idx
            .and_then(|idx| self.audio_tracks.get(idx))
            .is_some_and(|a| a.stream_index == stream_index);

        if is_video {
            if let Some(decoder) = unit.video.as_deref_mut() {
                match decoder.submit(AccessUnit {
                    data: &packet.data,
                    timestamp: pts,
                    metadata: u64::from(packet.is_keyframe),
                }) {
                    Ok(()) => {}
                    Err(DecoderError::Full) => return Some(false),
                    Err(err) => log::warn!("linux player: video submit error: {err}"),
                }
            }
            self.demuxer.advance();
            return Some(true);
        }
        if is_audio {
            if unit
                .seek_target
                .is_some_and(|target| pts.saturating_add(Duration::from_millis(25)) < target)
            {
                self.demuxer.advance();
                return Some(true);
            }
            if let Some(audio_dec) = unit.audio.as_deref_mut()
                && !audio_dec.send_packet(&packet.data)
            {
                return Some(false);
            }
            self.demuxer.advance();
            return Some(true);
        }
        self.demuxer.advance();
        Some(true)
    }

    fn replace_segment(&mut self, bytes: Arc<[u8]>, intra_seek: Duration) -> bool {
        let Ok(mut next_demux) = Demuxer::from_bytes(bytes) else {
            return false;
        };
        let _ = next_demux.inspect_tracks();
        if intra_seek > Duration::ZERO {
            drop(next_demux.seek_to(intra_seek));
        }
        self.demuxer = next_demux;
        true
    }

    fn seek_direct(&mut self, absolute_pos: Duration) {
        drop(self.demuxer.seek_to(absolute_pos));
    }

    fn switch_audio(&mut self, _prev: Option<usize>, next: Option<usize>) -> Option<Self::Audio> {
        self.active_audio_idx = next;
        next.and_then(|idx| self.audio_tracks.get(idx))
            .and_then(|track| self.demuxer.open_audio_decoder(track.stream_index).ok())
    }
}

fn open_demuxer(
    item: &MediaItem,
    selection: &TrackSelectionParameters,
    cancel: &FetchCancel,
) -> Result<(Demuxer, Option<AdaptiveStream>, Duration), PlayerError> {
    if let Some(bytes) = &item.bytes {
        let demuxer = Demuxer::from_bytes(Arc::clone(bytes)).map_err(PlayerError::Source)?;
        return Ok((demuxer, None, Duration::ZERO));
    }
    if let Some(path) = local_path_from_uri(&item.uri) {
        let demuxer = Demuxer::from_file(&path).map_err(PlayerError::Source)?;
        return Ok((demuxer, None, Duration::ZERO));
    }
    if is_http_uri(&item.uri) {
        if let Some(kind) = detect_manifest_kind(item) {
            let (adaptive, bytes, start_pts) =
                open_adaptive_first_segment(item, kind, selection, cancel.clone())?;
            let demuxer = Demuxer::from_bytes(bytes).map_err(PlayerError::Source)?;
            return Ok((demuxer, Some(adaptive), start_pts));
        }
        let reader = HttpRangeReader::open(&item.uri, item.headers.clone(), cancel.clone())
            .map_err(PlayerError::Network)?;
        let demuxer = Demuxer::from_http(reader).map_err(PlayerError::Network)?;
        return Ok((demuxer, None, Duration::ZERO));
    }
    Err(PlayerError::Unsupported(format!(
        "unsupported Linux media URI: {}",
        item.uri
    )))
}
