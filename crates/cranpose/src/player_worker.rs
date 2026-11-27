//! Shared off-UI playback worker, A/V clock presentation, adaptive segment handoff, and sidecar
//! subtitle runner reused across native [`PlayerBackend`] implementations (Android, Linux,
//! Windows).

use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AudioRole, CueGroup, DecodedFrames, FrameProducer, FrameTiming, LoadControl, MediaItem,
    MediaProperties, PlaybackParameters, PlayerBackend, PlayerBackendFactory, PlayerCapabilities,
    PlayerError, PlayerSink, PlayerState, TrackSelectionParameters, Tracks, VideoCodec,
    VideoContainer, VideoDecoder, VideoFrame, VideoSize, set_platform_player_factory,
};

use crate::{
    player_audio::PcmAudioOutput,
    player_control::{WorkerBackend, WorkerControl},
    player_stream::{
        AdaptiveStream, FetchCancel, LoadedSubtitle, ManifestKind, choose_subtitle_index,
        load_sidecar_subtitles, merge_sidecar_tracks,
    },
};

// Android's `AImageReader` reserves `depth` full GPU buffers and adds `depth` to MediaCodec's
// native-window `MIN_UNDEQUEUED_BUFFERS`; keeping it at 3 fits mobile OMX port buffer caps while
// still holding 1 presented frame + 2 queued frames for smooth 60/120 Hz pacing.
#[cfg(target_os = "android")]
pub(crate) const FRAME_QUEUE_DEPTH: usize = 3;
#[cfg(not(target_os = "android"))]
pub(crate) const FRAME_QUEUE_DEPTH: usize = 8;
pub(crate) const AUDIO_TARGET_BUFFER: Duration = Duration::from_millis(250);
const ACTIVE_STEP_WAIT: Duration = Duration::from_millis(4);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(120);

pub(crate) struct OpenedMedia<S> {
    pub(crate) media: S,
    pub(crate) adaptive: Option<AdaptiveStream>,
    pub(crate) segment_base_pts: Duration,
    pub(crate) has_audio: bool,
    pub(crate) video_size: Option<VideoSize>,
    pub(crate) tracks: Tracks,
    pub(crate) duration: Option<Duration>,
    pub(crate) is_live: bool,
    pub(crate) is_seekable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPresence {
    Present,
    Absent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseSessionMode {
    ReplaceItem,
    ClearOsSession,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaybackDrive {
    Playing,
    Paused,
}

impl<S> OpenedMedia<S> {
    pub(crate) fn from_adaptive(
        media: S,
        adaptive: Option<AdaptiveStream>,
        segment_base_pts: Duration,
        audio: AudioPresence,
        video_size: Option<VideoSize>,
        tracks: Tracks,
        fallback_duration: Option<Duration>,
    ) -> Self {
        let is_live = adaptive.as_ref().is_some_and(AdaptiveStream::is_live);
        let duration = adaptive
            .as_ref()
            .and_then(AdaptiveStream::duration)
            .or(fallback_duration);
        let is_seekable = adaptive
            .as_ref()
            .map_or_else(|| duration.is_some(), AdaptiveStream::is_seekable);
        Self {
            media,
            adaptive,
            segment_base_pts,
            has_audio: audio == AudioPresence::Present,
            video_size,
            tracks,
            duration,
            is_live,
            is_seekable,
        }
    }
}

pub(crate) struct StepContext<'a> {
    pub(crate) segment_base_pts: Duration,
    pub(crate) seek_target: Option<Duration>,
    pub(crate) pending_video: &'a mut VecDeque<VideoFrame>,
    pub(crate) audio: &'a mut PcmAudioOutput,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionPlaybackEvent {
    Playing,
    Paused,
    Stopped,
}

pub(crate) trait NativePlaybackEngine: Send + 'static {
    type Media;

    fn thread_name() -> &'static str;
    fn capabilities() -> PlayerCapabilities;

    fn on_thread_start(&mut self) -> Result<(), PlayerError> {
        Ok(())
    }

    fn open(
        &mut self,
        item: &MediaItem,
        initial_target: Duration,
        selection: &TrackSelectionParameters,
        cancel: &FetchCancel,
    ) -> Result<OpenedMedia<Self::Media>, PlayerError>;

    fn step(
        &mut self,
        media: &mut Self::Media,
        ctx: &mut StepContext<'_>,
    ) -> Result<bool, PlayerError>;

    fn drain_outputs(
        &mut self,
        _media: &mut Self::Media,
        _pending_video: &mut VecDeque<VideoFrame>,
        _audio: &mut PcmAudioOutput,
    ) {
    }

    fn replace_segment(
        &mut self,
        media: &mut Self::Media,
        bytes: Arc<[u8]>,
        intra_seek: Duration,
    ) -> bool;

    fn seek_direct(&mut self, media: &mut Self::Media, absolute_pos: Duration);

    fn flush_decoders(&mut self, _media: &mut Self::Media) {}

    fn drain_decoders(&mut self, _media: &mut Self::Media) {}

    fn apply_track_selection(
        &mut self,
        _media: &mut Self::Media,
        _selection: &TrackSelectionParameters,
        _audio: &mut PcmAudioOutput,
    ) {
    }

    fn buffered_position(&self, _media: &Self::Media, relative_pos: Duration) -> Duration {
        relative_pos
    }

    fn request_primary_focus(&mut self) -> bool {
        true
    }

    fn abandon_primary_focus(&mut self) {}

    fn notify_os_metadata(&mut self, _metadata: &cranpose_services::MediaMetadata) {}

    fn notify_os_session(
        &mut self,
        _event: SessionPlaybackEvent,
        _position: Duration,
        _duration: Option<Duration>,
        _speed: f32,
    ) {
    }
}

pub(crate) fn register_native_player<E: NativePlaybackEngine + Default>() {
    set_platform_player_factory(Arc::new(NativePlayerFactory::<E>(PhantomData)));
}

pub(crate) fn probe_decoder_capabilities(
    supported_containers: Vec<VideoContainer>,
) -> PlayerCapabilities {
    let mut supported_video_codecs = Vec::new();
    let mut hardware_video_codecs = Vec::new();
    let mut zero_copy_gpu = false;
    let mut hdr_10bit = false;
    for codec in [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Vp9,
        VideoCodec::Av1,
    ] {
        if let Ok(caps) = VideoDecoder::capabilities(codec) {
            supported_video_codecs.push(codec);
            if caps.hardware_accelerated == Some(true) {
                hardware_video_codecs.push(codec);
            }
            if caps.zero_copy_gpu {
                zero_copy_gpu = true;
            }
            if caps.hdr_10bit == Some(true) {
                hdr_10bit = true;
            }
        }
    }
    PlayerCapabilities {
        supported_containers,
        supported_video_codecs,
        hardware_video_codecs,
        zero_copy_gpu,
        hdr_10bit,
    }
}

pub(crate) fn open_adaptive_first_segment(
    item: &MediaItem,
    kind: ManifestKind,
    selection: &TrackSelectionParameters,
    cancel: FetchCancel,
) -> Result<(AdaptiveStream, Arc<[u8]>, Duration), PlayerError> {
    let mut adaptive =
        AdaptiveStream::open(item, kind, selection, cancel).map_err(PlayerError::Network)?;
    let first = adaptive
        .next_segment(selection)
        .map_err(PlayerError::Network)?
        .ok_or_else(|| PlayerError::Network("streaming manifest contains no segments".into()))?;
    Ok((adaptive, first.bytes, first.start_pts))
}

struct NativePlayerFactory<E>(PhantomData<fn() -> E>);

impl<E: NativePlaybackEngine + Default> PlayerBackendFactory for NativePlayerFactory<E> {
    fn create(
        &self,
        sink: PlayerSink,
        _load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        let cancel_slot = Arc::new(Mutex::new(FetchCancel::default()));
        let interrupt_slot = Arc::clone(&cancel_slot);
        let control = Arc::new(WorkerControl::new(Some(Arc::new(move || {
            interrupt_slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .cancel();
        }))));
        let worker_control = Arc::clone(&control);
        thread::Builder::new()
            .name(E::thread_name().into())
            .spawn(move || {
                let mut worker = NativePlayerWorker::new(
                    E::default(),
                    sink,
                    audio_role,
                    worker_control,
                    cancel_slot,
                );
                worker.run();
            })
            .map_err(|err| PlayerError::Source(format!("failed to spawn player worker: {err}")))?;
        Ok(Arc::new(WorkerBackend::new(control)))
    }

    fn capabilities(&self) -> PlayerCapabilities {
        E::capabilities()
    }
}

struct ActiveSession<M> {
    media: M,
    adaptive: Option<AdaptiveStream>,
    segment_base_pts: Duration,
    video_frames_out: FrameProducer,
    pending_video_frames: VecDeque<VideoFrame>,
    audio_output: PcmAudioOutput,
    duration: Option<Duration>,
    clip_start: Duration,
    clip_end: Option<Duration>,
    subtitles: Vec<LoadedSubtitle>,
    active_subtitle_index: Option<usize>,
    last_cue: Option<CueGroup>,
    last_progress_at: Instant,
    demux_eos_at: Option<Instant>,
    ended: bool,
    seek_target: Option<Duration>,
    last_pre_seek_frame: Option<VideoFrame>,
    focus_held: bool,
}

struct NativePlayerWorker<E: NativePlaybackEngine> {
    engine: E,
    sink: PlayerSink,
    audio_role: AudioRole,
    control: Arc<WorkerControl>,
    cancel_slot: Arc<Mutex<FetchCancel>>,
    session: Option<ActiveSession<E::Media>>,
    want_play: bool,
    volume: f32,
    parameters: PlaybackParameters,
    track_selection: TrackSelectionParameters,
}

impl<E: NativePlaybackEngine> NativePlayerWorker<E> {
    fn new(
        engine: E,
        sink: PlayerSink,
        audio_role: AudioRole,
        control: Arc<WorkerControl>,
        cancel_slot: Arc<Mutex<FetchCancel>>,
    ) -> Self {
        Self {
            engine,
            sink,
            audio_role,
            control,
            cancel_slot,
            session: None,
            want_play: false,
            volume: if audio_role == AudioRole::Muted {
                0.0
            } else {
                1.0
            },
            parameters: PlaybackParameters::DEFAULT,
            track_selection: TrackSelectionParameters::default(),
        }
    }

    fn effective_volume(&self) -> f32 {
        if self.audio_role == AudioRole::Muted {
            0.0
        } else {
            self.volume
        }
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
        if let Err(err) = self.engine.on_thread_start() {
            self.sink.report_error(err);
            return;
        }
        loop {
            if self.process_commands() {
                break;
            }
            self.step_session();
            let active = self
                .session
                .as_ref()
                .is_some_and(|s| !s.ended && (self.want_play || s.seek_target.is_some()));
            self.control
                .park_until(active.then_some(ACTIVE_STEP_WAIT), || false);
        }
        self.close_session(CloseSessionMode::ClearOsSession);
    }

    fn process_commands(&mut self) -> bool {
        let Some(batch) = self.control.take_batch() else {
            return true;
        };
        if batch.stop {
            self.want_play = false;
            self.close_session(CloseSessionMode::ClearOsSession);
        }
        if let Some(volume) = batch.volume {
            self.volume = volume;
            let eff = self.effective_volume();
            if let Some(s) = &mut self.session {
                s.audio_output.set_volume(eff);
            }
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
        self.close_session(CloseSessionMode::ReplaceItem);
        let cancel = self.reset_cancel();
        self.sink.set_loading(true);
        self.sink.set_playback_state(PlayerState::Buffering);

        let clip_start = item.clipping.map_or(Duration::ZERO, |c| c.start_position);
        let clip_end = item.clipping.and_then(|c| c.end_position);
        let initial_target = clip_start.saturating_add(start_position);

        let opened = match self
            .engine
            .open(item, initial_target, &self.track_selection, &cancel)
        {
            Ok(opened) => opened,
            Err(err) => {
                self.sink.set_loading(false);
                self.sink.report_error(err);
                return;
            }
        };

        if let Some(size) = opened.video_size {
            self.sink.set_video_size(size);
        }
        let raw_duration = opened.duration.or(item.metadata.duration);
        let effective_duration = match (raw_duration, clip_end) {
            (_, Some(end)) => Some(end.saturating_sub(clip_start)),
            (Some(dur), None) => Some(dur.saturating_sub(clip_start)),
            (None, None) => None,
        };
        let (video_frames_out, public_frames) = DecodedFrames::channel(FRAME_QUEUE_DEPTH);
        self.sink.set_decoded_frames(Some(public_frames));

        let subtitles = load_sidecar_subtitles(item, &cancel);
        let active_subtitle_index = choose_subtitle_index(&subtitles, &self.track_selection);
        let tracks = merge_sidecar_tracks(opened.tracks, &subtitles, active_subtitle_index);
        self.sink.set_tracks(tracks);
        self.sink.set_media_properties(MediaProperties {
            duration: effective_duration,
            is_seekable: opened.is_seekable || effective_duration.is_some(),
            is_live: opened.is_live,
        });
        if self.audio_role == AudioRole::Primary {
            self.engine.notify_os_metadata(&item.metadata);
        }

        let mut audio_output = PcmAudioOutput::new(self.effective_volume(), self.parameters.speed);
        audio_output.set_has_audio_track(opened.has_audio);
        audio_output.reset(initial_target);

        self.session = Some(ActiveSession {
            media: opened.media,
            adaptive: opened.adaptive,
            segment_base_pts: opened.segment_base_pts,
            video_frames_out,
            pending_video_frames: VecDeque::with_capacity(FRAME_QUEUE_DEPTH + 2),
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
            demux_eos_at: None,
            ended: false,
            seek_target: Some(initial_target),
            last_pre_seek_frame: None,
            focus_held: false,
        });
        self.sink.set_loading(false);
        self.sink.set_playback_state(PlayerState::Ready);
        self.publish_sidecar_cue(start_position);
        self.apply_play_state();
    }

    fn apply_parameters(&mut self) {
        let speed = self.parameters.speed;
        let is_primary = self.audio_role == AudioRole::Primary;
        let want_play = self.want_play;
        if let Some(s) = &mut self.session {
            s.audio_output.set_speed(speed);
            if is_primary {
                let pos = s.audio_output.position().saturating_sub(s.clip_start);
                let event = if want_play {
                    SessionPlaybackEvent::Playing
                } else {
                    SessionPlaybackEvent::Paused
                };
                self.engine.notify_os_session(event, pos, s.duration, speed);
            }
        }
    }

    fn apply_play_state(&mut self) {
        let is_primary = self.audio_role == AudioRole::Primary;
        let want_play = self.want_play;
        let speed = self.parameters.speed;
        let Some(s) = &mut self.session else {
            return;
        };
        if want_play && !s.ended {
            if is_primary && !s.focus_held {
                if !self.engine.request_primary_focus() {
                    self.want_play = false;
                    return;
                }
                s.focus_held = true;
            }
            s.audio_output.set_playing(true);
            if is_primary {
                let pos = s.audio_output.position().saturating_sub(s.clip_start);
                self.engine.notify_os_session(
                    SessionPlaybackEvent::Playing,
                    pos,
                    s.duration,
                    speed,
                );
            }
        } else {
            s.audio_output.set_playing(false);
            if is_primary {
                let pos = s.audio_output.position().saturating_sub(s.clip_start);
                self.engine
                    .notify_os_session(SessionPlaybackEvent::Paused, pos, s.duration, speed);
            }
        }
    }

    fn seek_session(&mut self, position: Duration) {
        let Some(s) = &mut self.session else {
            return;
        };
        let absolute = s.clip_start.saturating_add(position);
        s.ended = false;
        s.demux_eos_at = None;
        s.pending_video_frames.clear();
        s.last_pre_seek_frame = None;
        s.video_frames_out.clear();
        s.seek_target = Some(absolute);

        if let Some(adaptive) = s.adaptive.as_mut() {
            let intra = adaptive.seek_to(absolute);
            if let Ok(Some(seg)) = adaptive.next_segment(&self.track_selection)
                && self.engine.replace_segment(&mut s.media, seg.bytes, intra)
            {
                s.segment_base_pts = seg.start_pts;
            }
        } else {
            self.engine.seek_direct(&mut s.media, absolute);
        }
        self.engine.flush_decoders(&mut s.media);

        s.audio_output.reset(absolute);
        s.last_progress_at = Instant::now()
            .checked_sub(PROGRESS_INTERVAL)
            .unwrap_or_else(Instant::now);
        self.sink.set_playback_state(PlayerState::Ready);
        self.sink.set_progress(position, position);
        self.publish_sidecar_cue(position);
        self.apply_play_state();
    }

    fn apply_track_selection(&mut self) {
        let Some(s) = &mut self.session else {
            return;
        };
        if let Some(adaptive) = s.adaptive.as_mut() {
            adaptive.update_selection(&self.track_selection);
        }
        s.active_subtitle_index = choose_subtitle_index(&s.subtitles, &self.track_selection);
        self.engine
            .apply_track_selection(&mut s.media, &self.track_selection, &mut s.audio_output);
        let pos = s.audio_output.position().saturating_sub(s.clip_start);
        self.publish_sidecar_cue(pos);
    }

    fn step_session(&mut self) {
        if !self.advance_demux_and_drain() {
            return;
        }
        let Some(s) = &mut self.session else {
            return;
        };
        let drive = if self.want_play {
            PlaybackDrive::Playing
        } else {
            PlaybackDrive::Paused
        };
        advance_presentation(s, drive);

        let abs_pos = s.audio_output.position();
        let rel_pos = abs_pos.saturating_sub(s.clip_start);
        let hit_clip_end = s.clip_end.is_some_and(|end| abs_pos >= end);
        let drained = s.seek_target.is_none()
            && s.demux_eos_at
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
            && s.pending_video_frames.is_empty()
            && s.audio_output.buffered_duration() < Duration::from_millis(15);

        if hit_clip_end || drained {
            s.ended = true;
            s.audio_output.set_playing(false);
            let final_pos = s.duration.unwrap_or(rel_pos);
            self.sink.set_progress(final_pos, final_pos);
            self.sink.set_playback_state(PlayerState::Ended);
            if self.audio_role == AudioRole::Primary {
                self.engine.notify_os_session(
                    SessionPlaybackEvent::Paused,
                    final_pos,
                    s.duration,
                    0.0,
                );
            }
            return;
        }

        if s.last_progress_at.elapsed() >= PROGRESS_INTERVAL {
            s.last_progress_at = Instant::now();
            let buffered = self.engine.buffered_position(&s.media, rel_pos);
            self.sink.set_progress(rel_pos, buffered);
            self.publish_sidecar_cue(rel_pos);
        }
    }

    fn advance_demux_and_drain(&mut self) -> bool {
        let Some(s) = &mut self.session else {
            return false;
        };
        if s.ended || (!self.want_play && s.seek_target.is_none()) {
            return false;
        }
        s.audio_output.ensure_connected();
        let _: bool = s.audio_output.flush_pending();

        if s.demux_eos_at.is_none() {
            let mut ctx = StepContext {
                segment_base_pts: s.segment_base_pts,
                seek_target: s.seek_target,
                pending_video: &mut s.pending_video_frames,
                audio: &mut s.audio_output,
            };
            match self.engine.step(&mut s.media, &mut ctx) {
                Ok(true) => {
                    if let Some(adaptive) = s.adaptive.as_mut()
                        && let Ok(Some(seg)) = adaptive.next_segment(&self.track_selection)
                        && self
                            .engine
                            .replace_segment(&mut s.media, seg.bytes, Duration::ZERO)
                    {
                        s.segment_base_pts = seg.start_pts;
                    } else {
                        self.engine.drain_decoders(&mut s.media);
                        s.demux_eos_at = Some(Instant::now());
                    }
                }
                Ok(false) => {}
                Err(err) => {
                    self.sink.report_error(err);
                    return false;
                }
            }
        } else {
            self.engine.drain_outputs(
                &mut s.media,
                &mut s.pending_video_frames,
                &mut s.audio_output,
            );
        }
        true
    }

    fn publish_sidecar_cue(&mut self, position: Duration) {
        let Some(s) = &mut self.session else {
            return;
        };
        let Some(sub) = s.active_subtitle_index.and_then(|idx| s.subtitles.get(idx)) else {
            return;
        };
        let group = sub.track.cues_at(position);
        if s.last_cue.as_ref() != Some(&group) {
            s.last_cue = Some(group.clone());
            self.sink.set_cues(group);
        }
    }

    fn close_session(&mut self, mode: CloseSessionMode) {
        self.cancel_slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .cancel();
        if let Some(mut s) = self.session.take() {
            s.audio_output.set_playing(false);
            s.audio_output.close_device();
            s.video_frames_out.clear();
            if s.focus_held && self.audio_role == AudioRole::Primary {
                self.engine.abandon_primary_focus();
            }
            if mode == CloseSessionMode::ClearOsSession && self.audio_role == AudioRole::Primary {
                self.engine.notify_os_session(
                    SessionPlaybackEvent::Stopped,
                    Duration::ZERO,
                    None,
                    0.0,
                );
            }
        }
        self.sink.set_decoded_frames(None);
        self.sink.set_loading(false);
    }
}

fn advance_presentation<M>(s: &mut ActiveSession<M>, drive: PlaybackDrive) {
    if let Some(target) = s.seek_target {
        while let Some(front) = s.pending_video_frames.pop_front() {
            if front.info().timestamp >= target {
                s.last_pre_seek_frame = None;
                s.video_frames_out.push(front);
                s.seek_target = None;
                return;
            }
            s.last_pre_seek_frame = Some(front);
        }
        if s.demux_eos_at
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
        {
            if let Some(frame) = s.last_pre_seek_frame.take() {
                s.video_frames_out.push(frame);
            }
            s.seek_target = None;
        }
    } else if drive == PlaybackDrive::Playing {
        while let Some(front) = s.pending_video_frames.front() {
            match s.audio_output.frame_action(front.info().timestamp) {
                FrameTiming::Wait(_) => break,
                FrameTiming::PresentNow => {
                    if let Some(frame) = s.pending_video_frames.pop_front() {
                        s.video_frames_out.push(frame);
                    }
                }
                FrameTiming::DropLate(_) => {
                    drop(s.pending_video_frames.pop_front());
                }
            }
        }
    }
}
