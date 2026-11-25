#![expect(
    unsafe_code,
    reason = "AVPlayer, AVPlayerItemVideoOutput, and VTIsHardwareDecodeSupported are Apple C/Objective-C APIs"
)]

use std::{
    ptr::NonNull,
    sync::{Arc, Condvar, Mutex, Once, PoisonError, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use cranpose_services::{
    AudioRole, CueGroup, DecodedFrames, FrameInfo, FrameProducer, LoadControl, MediaItem,
    PlaybackParameters, PlayerBackend, PlayerBackendFactory, PlayerCapabilities, PlayerError,
    PlayerSink, PlayerState, TrackSelectionParameters, VideoCodec, VideoContainer, VideoRotation,
    set_platform_player_factory, video_frame_from_pixel_buffer_ptr,
};
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_av_foundation::{
    AVPlayer, AVPlayerItem, AVPlayerItemLegibleOutput, AVPlayerItemStatus, AVPlayerItemVideoOutput,
    AVPlayerTimeControlStatus,
};
use objc2_core_media::{
    CMTime, kCMVideoCodecType_AV1, kCMVideoCodecType_H264, kCMVideoCodecType_HEVC,
    kCMVideoCodecType_VP9,
};
use objc2_video_toolbox::VTIsHardwareDecodeSupported;

use super::player_media::{
    ItemObserver, ItemSignal, LegibleDelegate, LoadedSubtitle, OpenedAsset, apply_track_selection,
    buffered_position_of, build_video_output, cm_time, cm_time_to_duration, cm_time_zero,
    load_sidecar_subtitles, open_asset, probe_item_metadata,
};

const FRAME_QUEUE_DEPTH: usize = 4;
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(8);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const INITIAL_FRAME_WAIT_LIMIT: Duration = Duration::from_secs(2);

pub(crate) fn register() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        #[cfg(all(feature = "ios", target_os = "ios"))]
        super::player_route::install();
    });
    set_platform_player_factory(Arc::new(ApplePlayerFactory));
}

struct ApplePlayerFactory;

impl PlayerBackendFactory for ApplePlayerFactory {
    fn create(
        &self,
        sink: PlayerSink,
        load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        ApplePlayerBackend::spawn(sink, load_control, audio_role)
    }

    fn capabilities(&self) -> PlayerCapabilities {
        probe_apple_capabilities()
    }
}

fn probe_apple_capabilities() -> PlayerCapabilities {
    let mut supported_video_codecs = vec![VideoCodec::H264, VideoCodec::Hevc];
    let mut hardware_video_codecs = Vec::new();
    for (codec, fourcc) in [
        (VideoCodec::H264, kCMVideoCodecType_H264),
        (VideoCodec::Hevc, kCMVideoCodecType_HEVC),
        (VideoCodec::Vp9, kCMVideoCodecType_VP9),
        (VideoCodec::Av1, kCMVideoCodecType_AV1),
    ] {
        // SAFETY: VTIsHardwareDecodeSupported takes a codec FourCC by value and reads no caller memory.
        if unsafe { VTIsHardwareDecodeSupported(fourcc) } {
            hardware_video_codecs.push(codec);
            if !supported_video_codecs.contains(&codec) {
                supported_video_codecs.push(codec);
            }
        }
    }
    PlayerCapabilities {
        supported_containers: vec![
            VideoContainer::Mp4,
            VideoContainer::FragmentedMp4,
            VideoContainer::Hls,
        ],
        supported_video_codecs,
        hardware_video_codecs,
        hdr_10bit: true,
        zero_copy_gpu: true,
    }
}

struct PendingCommands {
    open: Option<(MediaItem, Duration)>,
    want_play: bool,
    play_changed: bool,
    stop: bool,
    seek: Option<Duration>,
    volume: f32,
    volume_changed: bool,
    parameters: PlaybackParameters,
    parameters_changed: bool,
    track_selection: TrackSelectionParameters,
    track_selection_changed: bool,
    released: bool,
}

impl Default for PendingCommands {
    fn default() -> Self {
        Self {
            open: None,
            want_play: false,
            play_changed: false,
            stop: false,
            seek: None,
            volume: 1.0,
            volume_changed: false,
            parameters: PlaybackParameters::DEFAULT,
            parameters_changed: false,
            track_selection: TrackSelectionParameters::default(),
            track_selection_changed: false,
            released: false,
        }
    }
}

struct SharedControl {
    commands: Mutex<PendingCommands>,
    wake_lock: Arc<Mutex<()>>,
    wake_cv: Arc<Condvar>,
}

impl SharedControl {
    fn lock_commands(&self) -> std::sync::MutexGuard<'_, PendingCommands> {
        self.commands.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wake(&self) {
        let _guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.wake_cv.notify_all();
    }
}

struct ApplePlayerBackend {
    control: Arc<SharedControl>,
}

impl ApplePlayerBackend {
    fn spawn(
        sink: PlayerSink,
        load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        let control = Arc::new(SharedControl {
            commands: Mutex::new(PendingCommands::default()),
            wake_lock: Arc::new(Mutex::new(())),
            wake_cv: Arc::new(Condvar::new()),
        });
        let worker_control = Arc::clone(&control);
        thread::Builder::new()
            .name("cranpose-apple-player".into())
            .spawn(move || {
                let mut worker =
                    ApplePlayerWorker::new(worker_control, sink, load_control, audio_role);
                worker.run();
            })
            .map_err(|err| PlayerError::Decoder(format!("spawn apple player worker: {err}")))?;
        Ok(Arc::new(Self { control }))
    }
}

impl Drop for ApplePlayerBackend {
    fn drop(&mut self) {
        self.control.lock_commands().released = true;
        self.control.wake();
    }
}

impl PlayerBackend for ApplePlayerBackend {
    fn open(&self, item: &MediaItem, start_position: Duration) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.open = Some((item.clone(), start_position));
        cmd.stop = false;
        cmd.seek = None;
        drop(cmd);
        self.control.wake();
    }

    fn play(&self) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.want_play = true;
        cmd.play_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn pause(&self) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.want_play = false;
        cmd.play_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn stop(&self) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.open = None;
        cmd.want_play = false;
        cmd.stop = true;
        cmd.seek = None;
        drop(cmd);
        self.control.wake();
    }

    fn seek_to(&self, position: Duration) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.seek = Some(position);
        drop(cmd);
        self.control.wake();
    }

    fn set_volume(&self, volume: f32) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.volume = volume.clamp(0.0, 1.0);
        cmd.volume_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn set_playback_parameters(&self, parameters: PlaybackParameters) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.parameters = parameters;
        cmd.parameters_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn set_track_selection(&self, parameters: &TrackSelectionParameters) {
        let mut cmd = self.control.lock_commands();
        if cmd.released {
            return;
        }
        cmd.track_selection = parameters.clone();
        cmd.track_selection_changed = true;
        drop(cmd);
        self.control.wake();
    }

    fn release(&self) {
        self.control.lock_commands().released = true;
        self.control.wake();
    }
}

struct ActiveSession {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    video_output: Retained<AVPlayerItemVideoOutput>,
    legible_output: Retained<AVPlayerItemLegibleOutput>,
    _legible_delegate: Retained<LegibleDelegate>,
    observer: Retained<ItemObserver>,
    signal: Arc<ItemSignal>,
    _opened_asset: OpenedAsset,
    producer: FrameProducer,
    rotation: VideoRotation,
    clip_start: Duration,
    stated_duration: Option<Duration>,
    sidecars: Vec<LoadedSubtitle>,
    active_sidecar: Option<usize>,
    metadata_published: bool,
    pending_frame_deadline: Option<Instant>,
    frame_sequence: u64,
    last_state: Option<PlayerState>,
    last_loading: Option<bool>,
    last_progress_at: Instant,
    last_cue: Option<CueGroup>,
}

impl ActiveSession {
    fn teardown(&mut self) {
        self.observer.detach();
        unsafe {
            self.player.pause();
            self.item.removeOutput(&self.video_output);
            self.item.removeOutput(&self.legible_output);
            self.player.replaceCurrentItemWithPlayerItem(None);
        }
        self.producer.clear();
    }
}

struct ApplePlayerWorker {
    control: Arc<SharedControl>,
    sink: PlayerSink,
    load_control: LoadControl,
    audio_role: AudioRole,
    session: Option<ActiveSession>,
    want_play: bool,
    volume: f32,
    parameters: PlaybackParameters,
    track_selection: TrackSelectionParameters,
}

impl ApplePlayerWorker {
    fn new(
        control: Arc<SharedControl>,
        sink: PlayerSink,
        load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Self {
        Self {
            control,
            sink,
            load_control,
            audio_role,
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

    fn run(&mut self) {
        loop {
            if self.process_commands() {
                break;
            }
            self.tick_session();
            let active = self.needs_active_tick();
            let wake_guard = self
                .control
                .wake_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !self.has_pending_work() {
                if active {
                    drop(
                        self.control
                            .wake_cv
                            .wait_timeout(wake_guard, ACTIVE_POLL_INTERVAL),
                    );
                } else {
                    drop(self.control.wake_cv.wait(wake_guard));
                }
            } else {
                drop(wake_guard);
            }
        }
        self.close_session(true);
    }

    fn has_pending_work(&self) -> bool {
        let has_cmd = {
            let cmd = self.control.lock_commands();
            cmd.released
                || cmd.stop
                || cmd.open.is_some()
                || cmd.play_changed
                || cmd.seek.is_some()
                || cmd.volume_changed
                || cmd.parameters_changed
                || cmd.track_selection_changed
        };
        if has_cmd {
            return true;
        }
        self.session.as_ref().is_some_and(|s| {
            s.signal.ended.load(Ordering::Acquire)
                || s.signal
                    .failed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_some()
        })
    }

    fn needs_active_tick(&self) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        if !session.metadata_published {
            return true;
        }
        if self.want_play && session.last_state != Some(PlayerState::Ended) {
            return true;
        }
        session
            .pending_frame_deadline
            .is_some_and(|deadline| Instant::now() < deadline)
    }

    fn process_commands(&mut self) -> bool {
        let snapshot = {
            let mut cmd = self.control.lock_commands();
            if cmd.released {
                return true;
            }
            PendingCommands {
                open: cmd.open.take(),
                want_play: cmd.want_play,
                play_changed: std::mem::take(&mut cmd.play_changed),
                stop: std::mem::take(&mut cmd.stop),
                seek: cmd.seek.take(),
                volume: cmd.volume,
                volume_changed: std::mem::take(&mut cmd.volume_changed),
                parameters: cmd.parameters,
                parameters_changed: std::mem::take(&mut cmd.parameters_changed),
                track_selection: if cmd.track_selection_changed {
                    cmd.track_selection.clone()
                } else {
                    TrackSelectionParameters::default()
                },
                track_selection_changed: std::mem::take(&mut cmd.track_selection_changed),
                released: false,
            }
        };

        if snapshot.volume_changed {
            self.volume = snapshot.volume;
            self.apply_volume();
        }
        if snapshot.parameters_changed {
            self.parameters = snapshot.parameters;
            self.apply_rate();
        }
        if snapshot.track_selection_changed {
            self.track_selection = snapshot.track_selection;
            if let Some(session) = &mut self.session {
                session.active_sidecar =
                    apply_track_selection(&session.item, &self.track_selection, &session.sidecars);
                let pos = cm_time_to_duration(unsafe { session.item.currentTime() })
                    .unwrap_or(Duration::ZERO)
                    .saturating_sub(session.clip_start);
                Self::publish_sidecar_cue(session, &self.sink, pos);
            }
        }
        if snapshot.stop {
            self.want_play = false;
            self.close_session(false);
        }
        if let Some((item, start_position)) = snapshot.open {
            self.open_item(&item, start_position);
        }
        if let Some(pos) = snapshot.seek {
            self.seek_session(pos);
        }
        if snapshot.play_changed {
            self.want_play = snapshot.want_play;
            self.apply_play_state();
        }
        false
    }

    fn open_item(&mut self, media_item: &MediaItem, start_position: Duration) {
        self.close_session(false);
        self.sink.set_loading(true);
        let opened_asset = match open_asset(media_item) {
            Ok(asset) => asset,
            Err(err) => {
                self.sink.report_error(err);
                return;
            }
        };
        let sidecars = load_sidecar_subtitles(media_item);
        let clip_start = media_item
            .clipping
            .map_or(Duration::ZERO, |clip| clip.start_position);
        let target_start = clip_start.saturating_add(start_position);
        let (producer, decoded_frames) = DecodedFrames::channel(FRAME_QUEUE_DEPTH);
        let signal = ItemSignal::new(
            Arc::clone(&self.control.wake_lock),
            Arc::clone(&self.control.wake_cv),
        );

        // SAFETY: This worker thread exclusively creates, mutates, and destroys its AVPlayer and
        // AVPlayerItem; Apple's AVFoundation permits off-main serialized usage.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let (player, item, video_output, legible_output, legible_delegate, observer) = unsafe {
            let item = AVPlayerItem::initWithAsset(AVPlayerItem::alloc(mtm), &opened_asset.asset);
            item.setPreferredForwardBufferDuration(self.load_control.max_buffer.as_secs_f64());
            if let Some(end) = media_item.clipping.and_then(|clip| clip.end_position) {
                item.setForwardPlaybackEndTime(cm_time(end));
            }
            let video_output = build_video_output();
            item.addOutput(&video_output);
            let (legible_output, legible_delegate) =
                LegibleDelegate::attach(&item, self.sink.clone());
            let observer = ItemObserver::attach(&item, Arc::clone(&signal));
            let player = AVPlayer::initWithPlayerItem(AVPlayer::alloc(mtm), Some(&item));
            player.setAutomaticallyWaitsToMinimizeStalling(true);
            player.setVolume(self.effective_volume());
            if !target_start.is_zero() {
                player.seekToTime_toleranceBefore_toleranceAfter(
                    cm_time(target_start),
                    cm_time_zero(),
                    cm_time_zero(),
                );
            }
            (
                player,
                item,
                video_output,
                legible_output,
                legible_delegate,
                observer,
            )
        };

        self.sink.set_decoded_frames(Some(decoded_frames));
        self.session = Some(ActiveSession {
            player,
            item,
            video_output,
            legible_output,
            _legible_delegate: legible_delegate,
            observer,
            signal,
            _opened_asset: opened_asset,
            producer,
            rotation: VideoRotation::Degrees0,
            clip_start,
            stated_duration: media_item.metadata.duration,
            sidecars,
            active_sidecar: None,
            metadata_published: false,
            pending_frame_deadline: Some(Instant::now() + INITIAL_FRAME_WAIT_LIMIT),
            frame_sequence: 0,
            last_state: Some(PlayerState::Buffering),
            last_loading: Some(true),
            last_progress_at: Instant::now() - PROGRESS_INTERVAL,
            last_cue: None,
        });
        if self.want_play {
            self.apply_play_state();
        }
    }

    fn close_session(&mut self, clear_frames: bool) {
        if let Some(mut session) = self.session.take() {
            session.teardown();
        }
        if clear_frames {
            self.sink.set_decoded_frames(None);
        }
        #[cfg(all(feature = "ios", target_os = "ios"))]
        if self.audio_role == AudioRole::Primary {
            let _ = crate::apple::audio_session::hold_for_media(false);
        }
    }

    fn seek_session(&mut self, position: Duration) {
        let Some(session) = &mut self.session else {
            return;
        };
        let target = session.clip_start.saturating_add(position);
        session.producer.clear();
        session.pending_frame_deadline = Some(Instant::now() + INITIAL_FRAME_WAIT_LIMIT);
        unsafe {
            session.player.seekToTime_toleranceBefore_toleranceAfter(
                cm_time(target),
                cm_time_zero(),
                cm_time_zero(),
            );
        }
        let buffered = buffered_position_of(&session.item).saturating_sub(session.clip_start);
        self.sink.set_progress(position, buffered);
        Self::publish_sidecar_cue(session, &self.sink, position);
        session.last_progress_at = Instant::now();
    }

    fn effective_volume(&self) -> f32 {
        if self.audio_role == AudioRole::Muted {
            0.0
        } else {
            self.volume
        }
    }

    fn apply_volume(&self) {
        if let Some(session) = &self.session {
            unsafe { session.player.setVolume(self.effective_volume()) };
        }
    }

    fn apply_rate(&self) {
        if let Some(session) = &self.session
            && self.want_play
        {
            unsafe { session.player.setRate(self.parameters.speed) };
        }
    }

    fn apply_play_state(&self) {
        let Some(session) = &self.session else {
            return;
        };
        #[cfg(all(feature = "ios", target_os = "ios"))]
        if self.audio_role == AudioRole::Primary {
            let _ = crate::apple::audio_session::hold_for_media(self.want_play);
        }
        unsafe {
            if self.want_play {
                if (self.parameters.speed - 1.0).abs() < f32::EPSILON {
                    session.player.play();
                } else {
                    session.player.setRate(self.parameters.speed);
                }
            } else {
                session.player.pause();
            }
        }
    }

    fn tick_session(&mut self) {
        let mut fatal_error: Option<PlayerError> = None;
        let mut ended = false;
        {
            let Some(session) = &mut self.session else {
                return;
            };
            if let Some(reason) = session
                .signal
                .failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                fatal_error = Some(PlayerError::Decoder(reason));
            } else if session.signal.ended.swap(false, Ordering::AcqRel) {
                ended = true;
            } else {
                let status = unsafe { session.item.status() };
                if status == AVPlayerItemStatus::Failed {
                    let msg = unsafe { session.item.error() }.map_or_else(
                        || "AVPlayerItem failed to load".to_owned(),
                        |err| err.localizedDescription().to_string(),
                    );
                    fatal_error = Some(PlayerError::Source(msg));
                } else if status == AVPlayerItemStatus::ReadyToPlay {
                    Self::handle_ready_tick(
                        session,
                        &self.sink,
                        &self.track_selection,
                        self.want_play,
                    );
                }
            }
        }
        if let Some(err) = fatal_error {
            self.want_play = false;
            self.close_session(false);
            self.sink.report_error(err);
        } else if ended {
            if let Some(session) = &mut self.session {
                session.last_state = Some(PlayerState::Ended);
                session.last_loading = Some(false);
                session.pending_frame_deadline = None;
            }
            self.sink.set_loading(false);
            self.sink.set_playback_state(PlayerState::Ended);
        }
    }

    fn handle_ready_tick(
        session: &mut ActiveSession,
        sink: &PlayerSink,
        track_selection: &TrackSelectionParameters,
        want_play: bool,
    ) {
        if !session.metadata_published {
            let (video_size, rotation, properties, tracks) =
                probe_item_metadata(&session.item, session.stated_duration, &session.sidecars);
            session.rotation = rotation;
            session.metadata_published = true;
            session.active_sidecar =
                apply_track_selection(&session.item, track_selection, &session.sidecars);
            sink.set_media_properties(properties);
            if video_size != cranpose_services::VideoSize::ZERO {
                sink.set_video_size(video_size);
            }
            if !tracks.is_empty() {
                sink.set_tracks(tracks);
            }
        }

        let time_status = unsafe { session.player.timeControlStatus() };
        let waiting = want_play
            && time_status == AVPlayerTimeControlStatus::WaitingToPlayAtSpecifiedRate
            && !unsafe { session.item.isPlaybackLikelyToKeepUp() };
        let next_state = if waiting {
            PlayerState::Buffering
        } else {
            PlayerState::Ready
        };
        if session.last_state != Some(next_state) {
            session.last_state = Some(next_state);
            sink.set_playback_state(next_state);
        }
        if session.last_loading != Some(waiting) {
            session.last_loading = Some(waiting);
            sink.set_loading(waiting);
        }

        let item_time = unsafe { session.item.currentTime() };
        Self::pull_video_frame(session, item_time);

        if session.last_progress_at.elapsed() >= PROGRESS_INTERVAL {
            let pos = cm_time_to_duration(item_time)
                .unwrap_or(Duration::ZERO)
                .saturating_sub(session.clip_start);
            let buffered = buffered_position_of(&session.item).saturating_sub(session.clip_start);
            sink.set_progress(pos, buffered);
            session.last_progress_at = Instant::now();
            Self::publish_sidecar_cue(session, sink, pos);
        }
    }

    fn publish_sidecar_cue(session: &mut ActiveSession, sink: &PlayerSink, pos: Duration) {
        let cue_group = session
            .active_sidecar
            .and_then(|idx| session.sidecars.get(idx))
            .map_or(CueGroup::EMPTY, |sub| sub.track.cues_at(pos));
        if session.last_cue.as_ref() != Some(&cue_group) {
            session.last_cue = Some(cue_group.clone());
            sink.set_cues(cue_group);
        }
    }

    fn pull_video_frame(session: &mut ActiveSession, item_time: CMTime) {
        let has_new = unsafe { session.video_output.hasNewPixelBufferForItemTime(item_time) };
        if !has_new {
            return;
        }
        let mut display_time = cm_time_zero();
        let pixel_buf = unsafe {
            session
                .video_output
                .copyPixelBufferForItemTime_itemTimeForDisplay(item_time, &mut display_time)
        };
        let Some(pixel_buf) = pixel_buf else {
            return;
        };
        let pts = cm_time_to_duration(display_time)
            .or_else(|| cm_time_to_duration(item_time))
            .unwrap_or(Duration::ZERO)
            .saturating_sub(session.clip_start);
        session.frame_sequence = session.frame_sequence.wrapping_add(1);
        let info = FrameInfo {
            timestamp: pts,
            metadata: session.frame_sequence,
            decode_time: Duration::ZERO,
        };
        let raw_ptr = NonNull::from(&*pixel_buf).as_ptr().cast();
        // SAFETY: `pixel_buf` is a live retained IOSurface-backed CVPixelBuffer;
        // `video_frame_from_pixel_buffer_ptr` retains its own CoreFoundation reference before
        // `pixel_buf` drops at the end of this function.
        if let Some(frame) =
            unsafe { video_frame_from_pixel_buffer_ptr(raw_ptr, info, session.rotation) }
        {
            session.producer.push(frame);
            session.pending_frame_deadline = None;
        }
    }
}
