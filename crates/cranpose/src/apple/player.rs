#![expect(
    unsafe_code,
    reason = "AVPlayer, AVPlayerItemVideoOutput, and VTIsHardwareDecodeSupported are Apple C/Objective-C APIs"
)]

use std::{
    ptr::NonNull,
    sync::{Arc, Once, PoisonError, atomic::Ordering},
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
use crate::player_control::{WorkerBackend, WorkerControl};

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
        let control = Arc::new(WorkerControl::new(None));
        let worker_control = Arc::clone(&control);
        thread::Builder::new()
            .name("cranpose-apple-player".into())
            .spawn(move || {
                let mut worker =
                    ApplePlayerWorker::new(worker_control, sink, load_control, audio_role);
                worker.run();
            })
            .map_err(|err| PlayerError::Decoder(format!("spawn apple player worker: {err}")))?;
        Ok(Arc::new(WorkerBackend::new(control)))
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
    control: Arc<WorkerControl>,
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
        control: Arc<WorkerControl>,
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
            let active_wait = self.needs_active_tick().then_some(ACTIVE_POLL_INTERVAL);
            self.control.park_until(active_wait, || {
                self.session.as_ref().is_some_and(|s| {
                    s.signal.ended.load(Ordering::Acquire)
                        || s.signal
                            .failed
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .is_some()
                })
            });
        }
        self.close_session(true);
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
        let Some(batch) = self.control.take_batch() else {
            return true;
        };

        if let Some(volume) = batch.volume {
            self.volume = volume;
            self.apply_volume();
        }
        if let Some(parameters) = batch.parameters {
            self.parameters = parameters;
            self.apply_rate();
        }
        if let Some(selection) = batch.track_selection {
            self.track_selection = selection;
            if let Some(session) = &mut self.session {
                session.active_sidecar =
                    apply_track_selection(&session.item, &self.track_selection, &session.sidecars);
                let pos = cm_time_to_duration(unsafe { session.item.currentTime() })
                    .unwrap_or(Duration::ZERO)
                    .saturating_sub(session.clip_start);
                Self::publish_sidecar_cue(session, &self.sink, pos);
            }
        }
        if batch.stop {
            self.want_play = false;
            self.close_session(false);
        }
        if let Some((item, start_position)) = batch.open {
            self.open_item(&item, start_position);
        }
        if let Some(pos) = batch.seek {
            self.seek_session(pos);
        }
        if let Some(want_play) = batch.play {
            self.want_play = want_play;
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
        let (wake_lock, wake_cv) = self.control.wake_pair();
        let signal = ItemSignal::new(wake_lock, wake_cv);

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
