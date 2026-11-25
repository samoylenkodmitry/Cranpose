// Shared `NativePlaybackEngine` adapter for platforms that demux packets in Rust and feed the
// shared zero-copy `VideoDecoder` + a platform audio decoder (Android NDK & Linux FFmpeg).

use std::{collections::VecDeque, marker::PhantomData, sync::Arc, time::Duration};

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, DecodeOrder, DecodedFrames, DecoderConfig,
    DecoderError, DecoderOutput, MediaItem, MediaMetadata, PlayerCapabilities, PlayerError,
    TrackEntry, TrackGroup, TrackSelectionParameters, TrackType, Tracks, VideoCodec,
    VideoColorInfo, VideoDecoder, VideoFrame, VideoRotation, VideoSize,
};

pub(crate) use crate::{
    player_audio::PcmAudioOutput,
    player_worker::{
        SessionPlaybackEvent, open_adaptive_first_segment, probe_decoder_capabilities,
    },
};
use crate::{
    player_control::select_track_index,
    player_stream::{AdaptiveStream, FetchCancel},
    player_worker::{
        AUDIO_TARGET_BUFFER, AudioPresence, FRAME_QUEUE_DEPTH, NativePlaybackEngine, OpenedMedia,
        StepContext, register_native_player,
    },
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoDecoderSpec {
    pub(crate) codec: VideoCodec,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rotation: VideoRotation,
    pub(crate) color: Option<VideoColorInfo>,
}

impl VideoDecoderSpec {
    fn video_size(&self) -> VideoSize {
        VideoSize::new(self.width, self.height)
    }
}

pub(crate) trait DemuxAudioDecoder {
    fn drain_pcm(&mut self, audio: &mut PcmAudioOutput);
    fn flush(&mut self);
    fn send_eos(&mut self);
}

pub(crate) struct FeedUnit<'a, A> {
    pub(crate) video: Option<&'a mut VideoDecoder>,
    pub(crate) audio: Option<&'a mut A>,
    pub(crate) segment_base_pts: Duration,
    pub(crate) seek_target: Option<Duration>,
}

pub(crate) struct OpenedDemux<D> {
    pub(crate) demux: D,
    pub(crate) adaptive: Option<AdaptiveStream>,
    pub(crate) segment_base_pts: Duration,
    pub(crate) video_spec: Option<VideoDecoderSpec>,
    pub(crate) audio_languages: Vec<Option<String>>,
    pub(crate) tracks: Tracks,
    pub(crate) duration: Option<Duration>,
}

pub(crate) trait PacketDemuxer: Sized + 'static {
    type Audio: DemuxAudioDecoder;

    fn thread_name() -> &'static str;
    fn decoder_label() -> &'static str;
    fn capabilities() -> PlayerCapabilities;

    fn open(
        item: &MediaItem,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedDemux<Self>, PlayerError>;

    fn feed_next_unit(&mut self, unit: &mut FeedUnit<'_, Self::Audio>) -> Option<bool>;
    fn replace_segment(&mut self, bytes: Arc<[u8]>, intra_seek: Duration) -> bool;
    fn seek_direct(&mut self, absolute_pos: Duration);
    fn switch_audio(&mut self, prev: Option<usize>, next: Option<usize>) -> Option<Self::Audio>;

    fn finalize_open(&mut self, initial_target: Duration) {
        if initial_target > Duration::ZERO {
            self.seek_direct(initial_target);
        }
    }

    fn buffered_position(&self, relative_pos: Duration) -> Duration {
        relative_pos
    }

    fn request_primary_focus() -> bool {
        true
    }

    fn abandon_primary_focus() {}

    fn notify_os_metadata(_metadata: &MediaMetadata) {}

    fn notify_os_session(
        _event: SessionPlaybackEvent,
        _position: Duration,
        _duration: Option<Duration>,
        _speed: f32,
    ) {
    }
}

pub(crate) fn register_demuxed_player<D: PacketDemuxer>() {
    register_native_player::<DemuxedEngine<D>>();
}

pub(crate) struct DemuxedEngine<D>(PhantomData<fn() -> D>);

impl<D> Default for DemuxedEngine<D> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

pub(crate) struct DemuxedMedia<D: PacketDemuxer> {
    demux: D,
    video_decoder: Option<VideoDecoder>,
    video_frames_in: Option<DecodedFrames>,
    audio_languages: Vec<Option<String>>,
    active_audio_idx: Option<usize>,
    audio_decoder: Option<D::Audio>,
}

impl<D: PacketDemuxer> NativePlaybackEngine for DemuxedEngine<D> {
    type Media = DemuxedMedia<D>;

    fn thread_name() -> &'static str {
        D::thread_name()
    }

    fn capabilities() -> PlayerCapabilities {
        D::capabilities()
    }

    fn open(
        &mut self,
        item: &MediaItem,
        initial_target: Duration,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedMedia<Self::Media>, PlayerError> {
        let mut opened = D::open(item, selection, cancel)?;
        let video_size = opened.video_spec.as_ref().map(VideoDecoderSpec::video_size);
        let (video_decoder, video_frames_in) =
            open_optional_video_decoder(opened.video_spec, D::decoder_label())?;
        let active_audio_idx = choose_audio_index(&opened.audio_languages, selection);
        let audio_decoder = opened.demux.switch_audio(None, active_audio_idx);
        opened.demux.finalize_open(initial_target);
        let audio = if audio_decoder.is_some() {
            AudioPresence::Present
        } else {
            AudioPresence::Absent
        };
        Ok(OpenedMedia::from_adaptive(
            DemuxedMedia {
                demux: opened.demux,
                video_decoder,
                video_frames_in,
                audio_languages: opened.audio_languages,
                active_audio_idx,
                audio_decoder,
            },
            opened.adaptive,
            opened.segment_base_pts,
            audio,
            video_size,
            opened.tracks,
            opened.duration,
        ))
    }

    fn step(
        &mut self,
        media: &mut Self::Media,
        ctx: &mut StepContext<'_>,
    ) -> Result<bool, PlayerError> {
        self.drain_outputs(media, ctx.pending_video, ctx.audio);
        let need_video =
            media.video_decoder.is_some() && ctx.pending_video.len() < FRAME_QUEUE_DEPTH;
        let need_audio =
            media.audio_decoder.is_some() && ctx.audio.buffered_duration() < AUDIO_TARGET_BUFFER;
        if need_video || need_audio {
            for _ in 0..6 {
                let mut unit = FeedUnit {
                    video: media.video_decoder.as_mut(),
                    audio: media.audio_decoder.as_mut(),
                    segment_base_pts: ctx.segment_base_pts,
                    seek_target: ctx.seek_target,
                };
                match media.demux.feed_next_unit(&mut unit) {
                    Some(true) => {}
                    Some(false) => break,
                    None => return Ok(true),
                }
            }
        }
        Ok(false)
    }

    fn drain_outputs(
        &mut self,
        media: &mut Self::Media,
        pending_video: &mut VecDeque<VideoFrame>,
        audio: &mut PcmAudioOutput,
    ) {
        if let Some(audio_dec) = media.audio_decoder.as_mut() {
            audio_dec.drain_pcm(audio);
        }
        if let Some(frames_in) = &media.video_frames_in {
            while pending_video.len() < FRAME_QUEUE_DEPTH + 2 {
                let Some(frame) = frames_in.next_frame() else {
                    break;
                };
                pending_video.push_back(frame);
            }
        }
    }

    fn replace_segment(
        &mut self,
        media: &mut Self::Media,
        bytes: Arc<[u8]>,
        intra_seek: Duration,
    ) -> bool {
        media.demux.replace_segment(bytes, intra_seek)
    }

    fn seek_direct(&mut self, media: &mut Self::Media, absolute_pos: Duration) {
        media.demux.seek_direct(absolute_pos);
    }

    fn flush_decoders(&mut self, media: &mut Self::Media) {
        if let Some(dec) = media.video_decoder.as_mut() {
            drop(dec.flush());
        }
        if let Some(audio) = media.audio_decoder.as_mut() {
            audio.flush();
        }
    }

    fn drain_decoders(&mut self, media: &mut Self::Media) {
        if let Some(dec) = media.video_decoder.as_mut() {
            drop(dec.drain());
        }
        if let Some(audio) = media.audio_decoder.as_mut() {
            audio.send_eos();
        }
    }

    fn apply_track_selection(
        &mut self,
        media: &mut Self::Media,
        selection: &TrackSelectionParameters,
        audio: &mut PcmAudioOutput,
    ) {
        let next_audio = choose_audio_index(&media.audio_languages, selection);
        if next_audio != media.active_audio_idx {
            media.audio_decoder = media.demux.switch_audio(media.active_audio_idx, next_audio);
            media.active_audio_idx = next_audio;
            audio.set_has_audio_track(media.audio_decoder.is_some());
        }
    }

    fn buffered_position(&self, media: &Self::Media, relative_pos: Duration) -> Duration {
        media.demux.buffered_position(relative_pos)
    }

    fn request_primary_focus(&mut self) -> bool {
        D::request_primary_focus()
    }

    fn abandon_primary_focus(&mut self) {
        D::abandon_primary_focus();
    }

    fn notify_os_metadata(&mut self, metadata: &MediaMetadata) {
        D::notify_os_metadata(metadata);
    }

    fn notify_os_session(
        &mut self,
        event: SessionPlaybackEvent,
        position: Duration,
        duration: Option<Duration>,
        speed: f32,
    ) {
        D::notify_os_session(event, position, duration, speed);
    }
}

fn choose_audio_index(
    languages: &[Option<String>],
    params: &TrackSelectionParameters,
) -> Option<usize> {
    select_track_index(
        languages.len(),
        |idx| languages[idx].as_deref(),
        TrackType::Audio,
        "audio",
        params,
    )
}

pub(crate) fn build_video_audio_tracks(
    video_entries: Vec<TrackEntry>,
    audio_entries: Vec<TrackEntry>,
) -> Tracks {
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
    Tracks { groups }
}

pub(crate) fn assemble_color_info(
    primaries: Option<ColorPrimaries>,
    range: Option<ColorRange>,
    transfer: Option<ColorTransfer>,
) -> Option<VideoColorInfo> {
    if primaries.is_none() && range.is_none() && transfer.is_none() {
        return None;
    }
    Some(VideoColorInfo {
        primaries: primaries.unwrap_or(ColorPrimaries::Bt709),
        range: range.unwrap_or(ColorRange::Limited),
        transfer: transfer.unwrap_or(ColorTransfer::Sdr),
    })
}

fn open_optional_video_decoder(
    spec: Option<VideoDecoderSpec>,
    platform_label: &str,
) -> Result<(Option<VideoDecoder>, Option<DecodedFrames>), PlayerError> {
    let Some(spec) = spec else {
        return Ok((None, None));
    };
    let mut cfg = DecoderConfig::new(
        spec.codec,
        spec.width,
        spec.height,
        DecoderOutput::Frames {
            depth: FRAME_QUEUE_DEPTH,
        },
    )
    .with_order(DecodeOrder::Presentation)
    .with_rotation(spec.rotation);
    cfg.input_wait = Duration::from_millis(5);
    if let Some(color) = spec.color {
        cfg = cfg.with_color(color);
    }
    let decoder = VideoDecoder::new(cfg).map_err(|err| match err {
        DecoderError::NoDecoder(_) | DecoderError::Unavailable => {
            PlayerError::Unsupported(format!("{platform_label} video decoder: {err}"))
        }
        other => PlayerError::Decoder(format!("{platform_label} video decoder: {other}")),
    })?;
    let frames = decoder.frames();
    Ok((Some(decoder), Some(frames)))
}
