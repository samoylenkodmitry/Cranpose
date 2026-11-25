//! Native zero-copy Android [`PlayerBackend`] built on NDK `AMediaExtractor` + `VideoDecoder`
//! (`AMediaCodec` -> `AImageReader` -> `AHardwareBuffer` -> Vulkan `YcbcrConverter`) + audio
//! `AMediaCodec` -> `cranpose-audio` AAudio anchored to `MasterClock` (<= 40 ms lip-sync).

use std::{
    collections::VecDeque,
    fs::File,
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AccessUnit, AudioRole, CueGroup, DecodeOrder, DecodedFrames, DecoderConfig, DecoderError,
    DecoderOutput, FrameProducer, FrameTiming, LoadControl, MediaItem, MediaProperties,
    PlaybackParameters, PlayerBackend, PlayerBackendFactory, PlayerCapabilities, PlayerError,
    PlayerSink, PlayerState, TrackSelectionParameters, TrackType, VideoCodec, VideoContainer,
    VideoDecoder, VideoFrame, set_platform_player_factory,
};

use super::{
    file_picker,
    media_session::{self, SESSION_PAUSED, SESSION_PLAYING, SESSION_STOPPED},
    player_extractor::{
        AudioCodecDecoder, AudioTrackInfo, DecodedAudio, ExtractedTracks, MediaExtractor,
        VideoTrackInfo, video_size_of,
    },
};
use crate::{
    player_audio::PcmAudioOutput,
    player_control::{WorkerBackend, WorkerControl, select_track_index},
    player_stream::{
        AdaptiveStream, FetchCancel, HttpRangeReader, LoadedSubtitle, choose_subtitle_index,
        detect_manifest_kind, is_http_uri, load_sidecar_subtitles, local_path_from_uri,
        merge_sidecar_tracks,
    },
};

const FRAME_QUEUE_DEPTH: usize = 8;
const ACTIVE_STEP_WAIT: Duration = Duration::from_millis(4);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);
const AUDIO_TARGET_BUFFER: Duration = Duration::from_millis(250);

pub(crate) fn register() {
    set_platform_player_factory(Arc::new(AndroidPlayerFactory));
}

struct AndroidPlayerFactory;

impl PlayerBackendFactory for AndroidPlayerFactory {
    fn create(
        &self,
        sink: PlayerSink,
        _load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        let cancel_slot = Arc::new(Mutex::new(FetchCancel::default()));
        let interrupt_slot = Arc::clone(&cancel_slot);
        let control = Arc::new(WorkerControl::new(Some(Arc::new(move || {
            let active = interrupt_slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            active.cancel();
        }))));
        let worker_control = Arc::clone(&control);
        thread::Builder::new()
            .name("cranpose-android-player".into())
            .spawn(move || {
                let mut worker =
                    AndroidPlayerWorker::new(sink, audio_role, worker_control, cancel_slot);
                worker.run();
            })
            .map_err(|err| {
                PlayerError::Source(format!("failed to spawn Android player worker: {err}"))
            })?;
        Ok(Arc::new(WorkerBackend::new(control)))
    }

    fn capabilities(&self) -> PlayerCapabilities {
        probe_android_capabilities()
    }
}

fn probe_android_capabilities() -> PlayerCapabilities {
    let mut supported_video_codecs = Vec::new();
    let mut hardware_video_codecs = Vec::new();
    let mut hdr_10bit = false;
    for codec in [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Vp9,
        VideoCodec::Av1,
    ] {
        if let Ok(caps) = VideoDecoder::capabilities(codec) {
            supported_video_codecs.push(codec);
            if caps.hardware_accelerated.unwrap_or(true) {
                hardware_video_codecs.push(codec);
            }
            if caps.hdr_10bit.unwrap_or(matches!(
                codec,
                VideoCodec::Hevc | VideoCodec::Vp9 | VideoCodec::Av1
            )) {
                hdr_10bit = true;
            }
        } else if super::player_extractor::ndk_has_decoder(codec.mime()) {
            supported_video_codecs.push(codec);
            if matches!(codec, VideoCodec::Hevc | VideoCodec::Vp9 | VideoCodec::Av1) {
                hdr_10bit = true;
            }
        }
    }
    if supported_video_codecs.is_empty() {
        supported_video_codecs.push(VideoCodec::H264);
    }
    PlayerCapabilities {
        supported_containers: vec![
            VideoContainer::Mp4,
            VideoContainer::FragmentedMp4,
            VideoContainer::Mkv,
            VideoContainer::Webm,
            VideoContainer::Hls,
            VideoContainer::Dash,
        ],
        supported_video_codecs,
        hardware_video_codecs,
        zero_copy_gpu: true,
        hdr_10bit,
    }
}

struct ActiveSession {
    extractor: MediaExtractor,
    adaptive: Option<AdaptiveStream>,
    segment_base_pts: Duration,
    video_info: Option<VideoTrackInfo>,
    video_decoder: Option<VideoDecoder>,
    video_frames_in: Option<DecodedFrames>,
    video_frames_out: FrameProducer,
    pending_video_frames: VecDeque<VideoFrame>,
    need_csd_prepend: bool,
    audio_tracks: Vec<AudioTrackInfo>,
    active_audio_idx: Option<usize>,
    audio_decoder: Option<AudioCodecDecoder>,
    audio_output: PcmAudioOutput,
    duration: Option<Duration>,
    clip_start: Duration,
    clip_end: Option<Duration>,
    subtitles: Vec<LoadedSubtitle>,
    active_subtitle_index: Option<usize>,
    last_cue: Option<CueGroup>,
    last_progress_at: Instant,
    demux_buf: Vec<u8>,
    packet_buf: Vec<u8>,
    demux_eos_at: Option<Instant>,
    ended: bool,
    seek_target: Option<Duration>,
    last_pre_seek_frame: Option<VideoFrame>,
    focus_held: bool,
}

struct AndroidPlayerWorker {
    sink: PlayerSink,
    audio_role: AudioRole,
    control: Arc<WorkerControl>,
    cancel_slot: Arc<Mutex<FetchCancel>>,
    session: Option<ActiveSession>,
    want_play: bool,
    volume: f32,
    parameters: PlaybackParameters,
    track_selection: TrackSelectionParameters,
}

impl AndroidPlayerWorker {
    fn new(
        sink: PlayerSink,
        audio_role: AudioRole,
        control: Arc<WorkerControl>,
        cancel_slot: Arc<Mutex<FetchCancel>>,
    ) -> Self {
        let initial_volume = if audio_role == AudioRole::Muted {
            0.0
        } else {
            1.0
        };
        Self {
            sink,
            audio_role,
            control,
            cancel_slot,
            session: None,
            want_play: false,
            volume: initial_volume,
            parameters: PlaybackParameters::DEFAULT,
            track_selection: TrackSelectionParameters::default(),
        }
    }

    fn cancel_in_flight(&self) {
        let active = self
            .cancel_slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        active.cancel();
    }

    fn reset_cancel(&self) -> FetchCancel {
        let next = FetchCancel::default();
        let prev = std::mem::replace(
            &mut *self
                .cancel_slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            next.clone(),
        );
        prev.cancel();
        next
    }

    fn run(&mut self) {
        loop {
            if self.process_commands() {
                break;
            }
            self.step_session();
            let active_wait = self.needs_active_tick().then_some(ACTIVE_STEP_WAIT);
            self.control.park_until(active_wait, || false);
        }
        self.close_session(true);
    }

    fn needs_active_tick(&self) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        if session.ended {
            return false;
        }
        self.want_play || session.seek_target.is_some()
    }

    fn process_commands(&mut self) -> bool {
        let Some(batch) = self.control.take_batch() else {
            return true;
        };
        if batch.stop {
            self.want_play = false;
            self.close_session(true);
        }
        if let Some(volume) = batch.volume {
            self.volume = volume;
            self.apply_volume();
        }
        if let Some(parameters) = batch.parameters {
            self.parameters = parameters;
            self.apply_parameters();
        }
        if let Some(selection) = batch.track_selection {
            self.track_selection = selection;
            self.apply_track_selection();
        }
        if let Some((item, start_position)) = batch.open {
            self.open_item(&item, start_position);
        }
        if let Some(seek_pos) = batch.seek {
            self.seek_session(seek_pos);
        }
        if let Some(want_play) = batch.play {
            self.want_play = want_play;
            self.apply_play_state();
        }
        false
    }

    fn open_item(&mut self, item: &MediaItem, start_position: Duration) {
        self.close_session(false);
        let cancel = self.reset_cancel();
        self.sink.set_loading(true);
        self.sink.set_playback_state(PlayerState::Buffering);

        let (extractor, adaptive, segment_base_pts) =
            match self.open_extractor_for_item(item, &cancel) {
                Ok(parts) => parts,
                Err(error) => {
                    self.sink.set_loading(false);
                    self.sink.report_error(error);
                    return;
                }
            };
        let session = match self.build_session(
            item,
            start_position,
            &cancel,
            extractor,
            adaptive,
            segment_base_pts,
        ) {
            Ok(session) => session,
            Err(error) => {
                self.sink.set_loading(false);
                self.sink.report_error(error);
                return;
            }
        };
        self.session = Some(session);
        self.sink.set_loading(false);
        self.sink.set_playback_state(PlayerState::Ready);
        self.publish_sidecar_cue(start_position);
        self.apply_play_state();
    }

    fn build_session(
        &self,
        item: &MediaItem,
        start_position: Duration,
        cancel: &FetchCancel,
        mut extractor: MediaExtractor,
        adaptive: Option<AdaptiveStream>,
        segment_base_pts: Duration,
    ) -> Result<ActiveSession, PlayerError> {
        let extracted = extractor.inspect_tracks();
        let clip_start = item.clipping.map_or(Duration::ZERO, |c| c.start_position);
        let clip_end = item.clipping.and_then(|c| c.end_position);
        let raw_duration = adaptive
            .as_ref()
            .and_then(AdaptiveStream::duration)
            .or(extracted.duration);
        let effective_duration = match (clip_end, raw_duration) {
            (Some(end), _) => Some(end.saturating_sub(clip_start)),
            (None, Some(dur)) => Some(dur.saturating_sub(clip_start)),
            (None, None) => None,
        };
        let is_live = adaptive.as_ref().is_some_and(AdaptiveStream::is_live);
        let is_seekable = adaptive
            .as_ref()
            .map_or_else(|| raw_duration.is_some(), AdaptiveStream::is_seekable);

        let (video_decoder, video_frames_in) = build_video_decoder(&extracted)?;
        if let Some(video) = &extracted.video {
            extractor
                .select_track(video.track_index)
                .map_err(PlayerError::Decoder)?;
            self.sink.set_video_size(video_size_of(video));
        }

        let active_audio_idx = choose_audio_track(&extracted.audio_tracks, &self.track_selection);
        let mut audio_decoder = None;
        if let Some(idx) = active_audio_idx
            && let Some(track) = extracted.audio_tracks.get(idx)
            && extractor.select_track(track.track_index).is_ok()
        {
            audio_decoder = extractor.open_audio_decoder(track.track_index).ok();
        }

        let (video_frames_out, public_frames) = DecodedFrames::channel(FRAME_QUEUE_DEPTH);
        self.sink.set_decoded_frames(Some(public_frames));

        let subtitles = load_sidecar_subtitles(item, cancel);
        let active_subtitle_index = choose_subtitle_index(&subtitles, &self.track_selection);
        let tracks = merge_sidecar_tracks(extracted.tracks, &subtitles, active_subtitle_index);
        self.sink.set_tracks(tracks);
        self.sink.set_media_properties(MediaProperties {
            duration: effective_duration,
            is_seekable,
            is_live,
        });
        if self.audio_role == AudioRole::Primary {
            media_session::set_metadata(&item.metadata);
        }

        let initial_target = clip_start.saturating_add(start_position);
        let mut audio_output = PcmAudioOutput::new(self.effective_volume(), self.parameters.speed);
        audio_output.set_has_audio_track(audio_decoder.is_some());
        audio_output.reset(initial_target);

        let mut session = ActiveSession {
            extractor,
            adaptive,
            segment_base_pts,
            video_info: extracted.video,
            video_decoder,
            video_frames_in,
            video_frames_out,
            pending_video_frames: VecDeque::with_capacity(FRAME_QUEUE_DEPTH + 2),
            need_csd_prepend: true,
            audio_tracks: extracted.audio_tracks,
            active_audio_idx,
            audio_decoder,
            audio_output,
            duration: effective_duration,
            clip_start,
            clip_end,
            subtitles,
            active_subtitle_index,
            last_cue: None,
            last_progress_at: Instant::now()
                .checked_sub(PROGRESS_INTERVAL)
                .unwrap_or_else(Instant::now),
            demux_buf: Vec::new(),
            packet_buf: Vec::new(),
            demux_eos_at: None,
            ended: false,
            seek_target: Some(initial_target),
            last_pre_seek_frame: None,
            focus_held: false,
        };
        if initial_target > Duration::ZERO {
            drop(session.extractor.seek_to(initial_target));
        }
        Ok(session)
    }

    fn open_extractor_for_item(
        &self,
        item: &MediaItem,
        cancel: &FetchCancel,
    ) -> Result<(MediaExtractor, Option<AdaptiveStream>, Duration), PlayerError> {
        if let Some(bytes) = &item.bytes {
            let extractor =
                MediaExtractor::from_bytes(Arc::clone(bytes)).map_err(PlayerError::Source)?;
            return Ok((extractor, None, Duration::ZERO));
        }
        if item.uri.starts_with("content://") {
            let file = file_picker::open_content_uri(&item.uri)
                .map_err(|e| PlayerError::Source(format!("content URI {}: {e}", item.uri)))?;
            let extractor = MediaExtractor::from_file(file).map_err(PlayerError::Source)?;
            return Ok((extractor, None, Duration::ZERO));
        }
        if let Some(path) = local_path_from_uri(&item.uri) {
            let file = File::open(&path)
                .map_err(|e| PlayerError::Source(format!("{}: {e}", path.display())))?;
            let extractor = MediaExtractor::from_file(file).map_err(PlayerError::Source)?;
            return Ok((extractor, None, Duration::ZERO));
        }
        if is_http_uri(&item.uri) {
            return self.open_http_extractor(item, cancel);
        }
        let extractor = MediaExtractor::from_uri(&item.uri).map_err(PlayerError::Source)?;
        Ok((extractor, None, Duration::ZERO))
    }

    fn open_http_extractor(
        &self,
        item: &MediaItem,
        cancel: &FetchCancel,
    ) -> Result<(MediaExtractor, Option<AdaptiveStream>, Duration), PlayerError> {
        if let Some(kind) = detect_manifest_kind(item) {
            let mut adaptive =
                AdaptiveStream::open(item, kind, &self.track_selection, cancel.clone())
                    .map_err(PlayerError::Network)?;
            let first = adaptive
                .next_segment(&self.track_selection)
                .map_err(PlayerError::Network)?
                .ok_or_else(|| {
                    PlayerError::Network("streaming manifest contains no segments".into())
                })?;
            let extractor = MediaExtractor::from_bytes(first.bytes).map_err(PlayerError::Source)?;
            return Ok((extractor, Some(adaptive), first.start_pts));
        }
        let reader = HttpRangeReader::open(&item.uri, item.headers.clone(), cancel.clone())
            .map_err(PlayerError::Network)?;
        let extractor = MediaExtractor::from_http(reader)
            .or_else(|_| MediaExtractor::from_uri(&item.uri))
            .map_err(PlayerError::Network)?;
        Ok((extractor, None, Duration::ZERO))
    }

    fn effective_volume(&self) -> f32 {
        if self.audio_role == AudioRole::Muted {
            0.0
        } else {
            self.volume
        }
    }

    fn apply_volume(&mut self) {
        let vol = self.effective_volume();
        if let Some(session) = &mut self.session {
            session.audio_output.set_volume(vol);
        }
    }

    fn apply_parameters(&mut self) {
        let speed = self.parameters.speed;
        let is_primary = self.audio_role == AudioRole::Primary;
        let want_play = self.want_play;
        if let Some(session) = &mut self.session {
            session.audio_output.set_speed(speed);
            if is_primary {
                let pos = session
                    .audio_output
                    .position()
                    .saturating_sub(session.clip_start);
                let state = if want_play {
                    SESSION_PLAYING
                } else {
                    SESSION_PAUSED
                };
                media_session::update_session(state, pos, session.duration, speed);
            }
        }
    }

    fn apply_play_state(&mut self) {
        let is_primary = self.audio_role == AudioRole::Primary;
        let want_play = self.want_play;
        let speed = self.parameters.speed;
        let Some(session) = &mut self.session else {
            return;
        };
        if want_play && !session.ended {
            if is_primary && !session.focus_held {
                if !media_session::request_focus() {
                    self.want_play = false;
                    return;
                }
                session.focus_held = true;
            }
            session.audio_output.set_playing(true);
            if is_primary {
                let pos = session
                    .audio_output
                    .position()
                    .saturating_sub(session.clip_start);
                media_session::update_session(SESSION_PLAYING, pos, session.duration, speed);
            }
        } else {
            session.audio_output.set_playing(false);
            if is_primary {
                let pos = session
                    .audio_output
                    .position()
                    .saturating_sub(session.clip_start);
                media_session::update_session(SESSION_PAUSED, pos, session.duration, speed);
            }
        }
    }

    fn seek_session(&mut self, position: Duration) {
        let Some(session) = &mut self.session else {
            return;
        };
        let absolute = session.clip_start.saturating_add(position);
        session.ended = false;
        session.demux_eos_at = None;
        session.pending_video_frames.clear();
        session.last_pre_seek_frame = None;
        session.video_frames_out.clear();
        session.need_csd_prepend = true;
        session.seek_target = Some(absolute);

        if let Some(adaptive) = session.adaptive.as_mut() {
            let intra = adaptive.seek_to(absolute);
            if let Ok(Some(seg)) = adaptive.next_segment(&self.track_selection)
                && let Ok(mut next_ext) = MediaExtractor::from_bytes(seg.bytes)
            {
                reselect_tracks(
                    &mut next_ext,
                    session.video_info.as_ref(),
                    &session.audio_tracks,
                    session.active_audio_idx,
                );
                if intra > Duration::ZERO {
                    drop(next_ext.seek_to(intra));
                }
                session.extractor = next_ext;
                session.segment_base_pts = seg.start_pts;
            }
        } else {
            drop(session.extractor.seek_to(absolute));
        }

        if let Some(dec) = session.video_decoder.as_mut() {
            drop(dec.flush());
        }
        if let Some(audio) = session.audio_decoder.as_mut() {
            audio.flush();
        }
        session.audio_output.reset(absolute);
        session.last_progress_at = Instant::now()
            .checked_sub(PROGRESS_INTERVAL)
            .unwrap_or_else(Instant::now);
        self.sink.set_playback_state(PlayerState::Ready);
        self.sink.set_progress(position, position);
        self.publish_sidecar_cue(position);
        self.apply_play_state();
    }

    fn apply_track_selection(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        let next_sub = choose_subtitle_index(&session.subtitles, &self.track_selection);
        session.active_subtitle_index = next_sub;
        if let Some(adaptive) = session.adaptive.as_mut() {
            adaptive.update_selection(&self.track_selection);
        }
        let next_audio = choose_audio_track(&session.audio_tracks, &self.track_selection);
        if next_audio != session.active_audio_idx {
            if let Some(prev) = session.active_audio_idx
                && let Some(track) = session.audio_tracks.get(prev)
            {
                session.extractor.unselect_track(track.track_index);
            }
            session.audio_decoder = None;
            if let Some(next) = next_audio
                && let Some(track) = session.audio_tracks.get(next)
                && session.extractor.select_track(track.track_index).is_ok()
            {
                session.audio_decoder =
                    session.extractor.open_audio_decoder(track.track_index).ok();
            }
            session.active_audio_idx = next_audio;
            session
                .audio_output
                .set_has_audio_track(session.audio_decoder.is_some());
        }
        let pos = session
            .audio_output
            .position()
            .saturating_sub(session.clip_start);
        self.publish_sidecar_cue(pos);
    }

    fn step_session(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        if session.ended || (!self.want_play && session.seek_target.is_none()) {
            return;
        }
        session.audio_output.ensure_connected();
        let _: bool = session.audio_output.flush_pending();
        drain_decoded_audio(session);
        pull_decoded_video(session);
        pump_demuxer(session, &self.track_selection);
        advance_presentation(session, self.want_play);

        let abs_pos = session.audio_output.position();
        let rel_pos = abs_pos.saturating_sub(session.clip_start);
        let hit_clip_end = session.clip_end.is_some_and(|end| abs_pos >= end);
        let drained = session.seek_target.is_none()
            && session
                .demux_eos_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
            && session.pending_video_frames.is_empty()
            && session.audio_output.buffered_duration() < Duration::from_millis(15);

        if hit_clip_end || drained {
            session.ended = true;
            session.audio_output.set_playing(false);
            let final_pos = session.duration.unwrap_or(rel_pos);
            self.sink.set_progress(final_pos, final_pos);
            self.sink.set_playback_state(PlayerState::Ended);
            if self.audio_role == AudioRole::Primary {
                media_session::update_session(SESSION_PAUSED, final_pos, session.duration, 0.0);
            }
            return;
        }

        if session.last_progress_at.elapsed() >= PROGRESS_INTERVAL {
            session.last_progress_at = Instant::now();
            let buffered = session
                .extractor
                .cached_duration()
                .map_or(rel_pos, |cached| rel_pos.saturating_add(cached));
            self.sink.set_progress(rel_pos, buffered);
            self.publish_sidecar_cue(rel_pos);
        }
    }

    fn publish_sidecar_cue(&mut self, position: Duration) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(sub_idx) = session.active_subtitle_index else {
            return;
        };
        let Some(sub) = session.subtitles.get(sub_idx) else {
            return;
        };
        let group = sub.track.cues_at(position);
        if session.last_cue.as_ref() != Some(&group) {
            session.last_cue = Some(group.clone());
            self.sink.set_cues(group);
        }
    }

    fn close_session(&mut self, clear_session: bool) {
        self.cancel_in_flight();
        if let Some(mut session) = self.session.take() {
            session.audio_output.set_playing(false);
            session.audio_output.close_device();
            session.video_frames_out.clear();
            if session.focus_held && self.audio_role == AudioRole::Primary {
                media_session::abandon_focus();
            }
            if clear_session && self.audio_role == AudioRole::Primary {
                media_session::update_session(SESSION_STOPPED, Duration::ZERO, None, 0.0);
            }
        }
        self.sink.set_decoded_frames(None);
        self.sink.set_loading(false);
    }
}

fn build_video_decoder(
    extracted: &ExtractedTracks,
) -> Result<(Option<VideoDecoder>, Option<DecodedFrames>), PlayerError> {
    let Some(video) = &extracted.video else {
        return Ok((None, None));
    };
    let mut cfg = DecoderConfig::new(
        video.codec,
        video.width,
        video.height,
        DecoderOutput::Frames {
            depth: FRAME_QUEUE_DEPTH,
        },
    )
    .with_order(DecodeOrder::Presentation)
    .with_rotation(video.rotation);
    cfg.input_wait = Duration::from_millis(5);
    if let Some(color) = video.color {
        cfg = cfg.with_color(color);
    }
    let decoder = VideoDecoder::new(cfg).map_err(|err| match err {
        DecoderError::NoDecoder(_) | DecoderError::Unavailable => {
            PlayerError::Unsupported(format!("Android hardware video decoder: {err}"))
        }
        other => PlayerError::Decoder(format!("Android hardware video decoder: {other}")),
    })?;
    let frames = decoder.frames();
    Ok((Some(decoder), Some(frames)))
}

fn feed_next_sample(session: &mut ActiveSession, selection: &TrackSelectionParameters) -> bool {
    let Some(sample) = session.extractor.peek_sample(&mut session.demux_buf) else {
        return handle_demux_eof(session, selection);
    };
    let pts = session.segment_base_pts.saturating_add(sample.pts);
    if session
        .video_info
        .as_ref()
        .is_some_and(|v| v.track_index == sample.track_index)
    {
        return feed_video_sample(session, sample.size, pts, sample.is_sync);
    }
    if session
        .active_audio_idx
        .and_then(|idx| session.audio_tracks.get(idx))
        .is_some_and(|a| a.track_index == sample.track_index)
    {
        return feed_audio_sample(session, sample.size, pts);
    }
    session.extractor.advance();
    true
}

fn handle_demux_eof(session: &mut ActiveSession, selection: &TrackSelectionParameters) -> bool {
    if let Some(adaptive) = session.adaptive.as_mut()
        && let Ok(Some(seg)) = adaptive.next_segment(selection)
        && let Ok(mut next_ext) = MediaExtractor::from_bytes(seg.bytes)
    {
        reselect_tracks(
            &mut next_ext,
            session.video_info.as_ref(),
            &session.audio_tracks,
            session.active_audio_idx,
        );
        session.extractor = next_ext;
        session.segment_base_pts = seg.start_pts;
        session.need_csd_prepend = true;
        return true;
    }
    session.demux_eos_at = Some(Instant::now());
    if let Some(dec) = session.video_decoder.as_mut() {
        drop(dec.drain());
    }
    if let Some(audio) = session.audio_decoder.as_mut() {
        let _: bool = audio.queue_eos();
    }
    false
}

fn feed_video_sample(
    session: &mut ActiveSession,
    size: usize,
    pts: Duration,
    is_sync: bool,
) -> bool {
    let payload = &session.demux_buf[..size];
    if let (Some(video_info), Some(decoder)) = (&session.video_info, session.video_decoder.as_mut())
    {
        let needs_csd = matches!(video_info.codec, VideoCodec::H264 | VideoCodec::Hevc)
            && !video_info.csd.is_empty()
            && (session.need_csd_prepend || is_sync);
        let unit_bytes = if needs_csd {
            session.packet_buf.clear();
            session.packet_buf.extend_from_slice(&video_info.csd);
            session.packet_buf.extend_from_slice(payload);
            session.packet_buf.as_slice()
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
            session.need_csd_prepend = false;
        }
    }
    session.extractor.advance();
    true
}

fn feed_audio_sample(session: &mut ActiveSession, size: usize, pts: Duration) -> bool {
    if session
        .seek_target
        .is_some_and(|target| pts.saturating_add(Duration::from_millis(25)) < target)
    {
        session.extractor.advance();
        return true;
    }
    let payload = &session.demux_buf[..size];
    if let Some(audio_dec) = session.audio_decoder.as_mut()
        && !audio_dec.queue_input(payload, pts)
    {
        return false;
    }
    session.extractor.advance();
    true
}

fn drain_decoded_audio(session: &mut ActiveSession) {
    let Some(audio_dec) = session.audio_decoder.as_mut() else {
        return;
    };
    while let Some(pcm) = audio_dec.dequeue_output() {
        match pcm {
            DecodedAudio::Pcm16 {
                samples,
                sample_rate,
                channels,
            } => session
                .audio_output
                .push_pcm_i16(&samples, sample_rate, channels),
            DecodedAudio::PcmFloat {
                samples,
                sample_rate,
                channels,
            } => session
                .audio_output
                .push_pcm_f32(&samples, sample_rate, channels),
        }
    }
}

fn pull_decoded_video(session: &mut ActiveSession) {
    let Some(frames_in) = &session.video_frames_in else {
        return;
    };
    while session.pending_video_frames.len() < FRAME_QUEUE_DEPTH + 2 {
        let Some(frame) = frames_in.next_frame() else {
            break;
        };
        session.pending_video_frames.push_back(frame);
    }
}

fn pump_demuxer(session: &mut ActiveSession, selection: &TrackSelectionParameters) {
    let need_more_video =
        session.video_decoder.is_some() && session.pending_video_frames.len() < FRAME_QUEUE_DEPTH;
    let need_more_audio = session.audio_decoder.is_some()
        && session.audio_output.buffered_duration() < AUDIO_TARGET_BUFFER;
    if session.demux_eos_at.is_some() || (!need_more_video && !need_more_audio) {
        return;
    }
    for _ in 0..6 {
        if !feed_next_sample(session, selection) {
            break;
        }
    }
}

fn advance_presentation(session: &mut ActiveSession, want_play: bool) {
    if let Some(target) = session.seek_target {
        while let Some(front) = session.pending_video_frames.pop_front() {
            if front.info().timestamp >= target {
                session.last_pre_seek_frame = None;
                session.video_frames_out.push(front);
                session.seek_target = None;
                return;
            }
            session.last_pre_seek_frame = Some(front);
        }
        if session
            .demux_eos_at
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
        {
            if let Some(frame) = session.last_pre_seek_frame.take() {
                session.video_frames_out.push(frame);
            }
            session.seek_target = None;
        }
    } else if want_play {
        present_due_frames(session);
    }
}

fn present_due_frames(session: &mut ActiveSession) {
    while let Some(front) = session.pending_video_frames.front() {
        let pts = front.info().timestamp;
        match session.audio_output.frame_action(pts) {
            FrameTiming::Wait(_) => break,
            FrameTiming::PresentNow => {
                if let Some(frame) = session.pending_video_frames.pop_front() {
                    session.video_frames_out.push(frame);
                }
            }
            FrameTiming::DropLate(_) => {
                drop(session.pending_video_frames.pop_front());
            }
        }
    }
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

fn choose_audio_track(
    tracks: &[AudioTrackInfo],
    params: &TrackSelectionParameters,
) -> Option<usize> {
    select_track_index(
        tracks.len(),
        |idx| tracks[idx].language.as_deref(),
        TrackType::Audio,
        "audio",
        params,
    )
}
