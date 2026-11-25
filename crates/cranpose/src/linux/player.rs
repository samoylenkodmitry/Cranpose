//! Native zero-copy Linux [`PlayerBackend`] built on runtime-loaded `libavformat`/`libavcodec` +
//! Phase 1 VA-API DMABUF `VideoDecoder` (with software fallback) + `cranpose-audio` cpal output
//! anchored to `MasterClock` (<= 40 ms lip-sync).

use std::{
    collections::VecDeque,
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

use super::player_demux::{
    AudioDecoder, AudioTrackInfo, DemuxedTracks, Demuxer, VideoTrackInfo, avformat_available,
    video_size_of,
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
    set_platform_player_factory(Arc::new(LinuxPlayerFactory));
}

struct LinuxPlayerFactory;

impl PlayerBackendFactory for LinuxPlayerFactory {
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
            .name("cranpose-linux-player".into())
            .spawn(move || {
                let mut worker =
                    LinuxPlayerWorker::new(sink, audio_role, worker_control, cancel_slot);
                worker.run();
            })
            .map_err(|err| {
                PlayerError::Source(format!("failed to spawn Linux player worker: {err}"))
            })?;
        Ok(Arc::new(WorkerBackend::new(control)))
    }

    fn capabilities(&self) -> PlayerCapabilities {
        probe_linux_capabilities()
    }
}

fn probe_linux_capabilities() -> PlayerCapabilities {
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
    let supported_containers = if avformat_available() {
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
    PlayerCapabilities {
        supported_containers,
        supported_video_codecs,
        hardware_video_codecs,
        zero_copy_gpu,
        hdr_10bit,
    }
}

struct ActiveSession {
    demuxer: Demuxer,
    adaptive: Option<AdaptiveStream>,
    segment_base_pts: Duration,
    video_info: Option<VideoTrackInfo>,
    video_decoder: Option<VideoDecoder>,
    video_frames_in: Option<DecodedFrames>,
    video_frames_out: FrameProducer,
    pending_video_frames: VecDeque<VideoFrame>,
    audio_tracks: Vec<AudioTrackInfo>,
    active_audio_idx: Option<usize>,
    audio_decoder: Option<AudioDecoder>,
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
}

struct LinuxPlayerWorker {
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

impl LinuxPlayerWorker {
    fn new(
        sink: PlayerSink,
        audio_role: AudioRole,
        control: Arc<WorkerControl>,
        cancel_slot: Arc<Mutex<FetchCancel>>,
    ) -> Self {
        Self {
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
            let active_wait = self.is_active().then_some(ACTIVE_STEP_WAIT);
            self.control.park_until(active_wait, || false);
        }
        self.close_session();
    }

    fn is_active(&self) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        !session.ended && (self.want_play || session.seek_target.is_some())
    }

    fn process_commands(&mut self) -> bool {
        let Some(batch) = self.control.take_batch() else {
            return true;
        };
        if batch.stop {
            self.want_play = false;
            self.close_session();
        }
        if let Some(volume) = batch.volume {
            self.volume = volume;
            let vol = self.effective_volume();
            if let Some(session) = &mut self.session {
                session.audio_output.set_volume(vol);
            }
        }
        if let Some(parameters) = batch.parameters {
            self.parameters = parameters;
            if let Some(session) = &mut self.session {
                session.audio_output.set_speed(parameters.speed);
            }
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
        self.close_session();
        let cancel = self.reset_cancel();
        self.sink.set_loading(true);
        self.sink.set_playback_state(PlayerState::Buffering);

        let (demuxer, adaptive, segment_base_pts) = match self.open_demuxer(item, &cancel) {
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
            demuxer,
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

    fn open_demuxer(
        &self,
        item: &MediaItem,
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
                let mut adaptive =
                    AdaptiveStream::open(item, kind, &self.track_selection, cancel.clone())
                        .map_err(PlayerError::Network)?;
                let first = adaptive
                    .next_segment(&self.track_selection)
                    .map_err(PlayerError::Network)?
                    .ok_or_else(|| {
                        PlayerError::Network("streaming manifest contains no segments".into())
                    })?;
                let demuxer = Demuxer::from_bytes(first.bytes).map_err(PlayerError::Source)?;
                return Ok((demuxer, Some(adaptive), first.start_pts));
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

    fn build_session(
        &self,
        item: &MediaItem,
        start_position: Duration,
        cancel: &FetchCancel,
        mut demuxer: Demuxer,
        adaptive: Option<AdaptiveStream>,
        segment_base_pts: Duration,
    ) -> Result<ActiveSession, PlayerError> {
        let extracted = demuxer.inspect_tracks();
        let (video_decoder, video_frames_in) = build_video_decoder(&extracted)?;
        if let Some(video) = &extracted.video {
            self.sink.set_video_size(video_size_of(video));
        }

        let active_audio_idx = choose_audio_track(&extracted.audio_tracks, &self.track_selection);
        let audio_decoder = active_audio_idx
            .and_then(|idx| extracted.audio_tracks.get(idx))
            .and_then(|track| demuxer.open_audio_decoder(track.stream_index).ok());

        let clip_start = item
            .clipping
            .map_or(Duration::ZERO, |clip| clip.start_position);
        let clip_end = item.clipping.and_then(|clip| clip.end_position);
        let is_live = adaptive.as_ref().is_some_and(AdaptiveStream::is_live);
        let raw_duration = adaptive
            .as_ref()
            .and_then(AdaptiveStream::duration)
            .or(extracted.duration)
            .or(item.metadata.duration);
        let effective_duration = match (raw_duration, clip_end) {
            (_, Some(end)) => Some(end.saturating_sub(clip_start)),
            (Some(dur), None) => Some(dur.saturating_sub(clip_start)),
            (None, None) => None,
        };
        let is_seekable = adaptive
            .as_ref()
            .map_or_else(|| effective_duration.is_some(), AdaptiveStream::is_seekable);

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

        let initial_target = clip_start.saturating_add(start_position);
        let mut audio_output = PcmAudioOutput::new(self.effective_volume(), self.parameters.speed);
        audio_output.set_has_audio_track(audio_decoder.is_some());
        audio_output.reset(initial_target);

        let mut session = ActiveSession {
            demuxer,
            adaptive,
            segment_base_pts,
            video_info: extracted.video,
            video_decoder,
            video_frames_in,
            video_frames_out,
            pending_video_frames: VecDeque::with_capacity(FRAME_QUEUE_DEPTH + 2),
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
            demux_eos_at: None,
            ended: false,
            seek_target: Some(initial_target),
            last_pre_seek_frame: None,
        };
        if initial_target > Duration::ZERO {
            drop(session.demuxer.seek_to(initial_target));
        }
        Ok(session)
    }

    fn effective_volume(&self) -> f32 {
        if self.audio_role == AudioRole::Muted {
            0.0
        } else {
            self.volume
        }
    }

    fn apply_play_state(&mut self) {
        let want_play = self.want_play;
        if let Some(session) = &mut self.session {
            session.audio_output.set_playing(want_play && !session.ended);
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
        session.seek_target = Some(absolute);

        if let Some(adaptive) = session.adaptive.as_mut() {
            let intra = adaptive.seek_to(absolute);
            if let Ok(Some(seg)) = adaptive.next_segment(&self.track_selection)
                && let Ok(mut next_demux) = Demuxer::from_bytes(seg.bytes)
            {
                let _ = next_demux.inspect_tracks();
                if intra > Duration::ZERO {
                    drop(next_demux.seek_to(intra));
                }
                session.demuxer = next_demux;
                session.segment_base_pts = seg.start_pts;
            }
        } else {
            drop(session.demuxer.seek_to(absolute));
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
        session.active_subtitle_index =
            choose_subtitle_index(&session.subtitles, &self.track_selection);
        if let Some(adaptive) = session.adaptive.as_mut() {
            adaptive.update_selection(&self.track_selection);
        }
        let next_audio = choose_audio_track(&session.audio_tracks, &self.track_selection);
        if next_audio != session.active_audio_idx {
            session.audio_decoder = next_audio
                .and_then(|idx| session.audio_tracks.get(idx))
                .and_then(|track| session.demuxer.open_audio_decoder(track.stream_index).ok());
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
            return;
        }

        if session.last_progress_at.elapsed() >= PROGRESS_INTERVAL {
            session.last_progress_at = Instant::now();
            self.sink.set_progress(rel_pos, rel_pos);
            self.publish_sidecar_cue(rel_pos);
        }
    }

    fn publish_sidecar_cue(&mut self, position: Duration) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(sub) = session
            .active_subtitle_index
            .and_then(|idx| session.subtitles.get(idx))
        else {
            return;
        };
        let group = sub.track.cues_at(position);
        if session.last_cue.as_ref() != Some(&group) {
            session.last_cue = Some(group.clone());
            self.sink.set_cues(group);
        }
    }

    fn close_session(&mut self) {
        let active = self
            .cancel_slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        active.cancel();
        if let Some(mut session) = self.session.take() {
            session.audio_output.set_playing(false);
            session.audio_output.close_device();
            session.video_frames_out.clear();
        }
        self.sink.set_decoded_frames(None);
        self.sink.set_loading(false);
    }
}

fn build_video_decoder(
    extracted: &DemuxedTracks,
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
            PlayerError::Unsupported(format!("Linux video decoder: {err}"))
        }
        other => PlayerError::Decoder(format!("Linux video decoder: {other}")),
    })?;
    let frames = decoder.frames();
    Ok((Some(decoder), Some(frames)))
}

fn drain_decoded_audio(session: &mut ActiveSession) {
    let Some(audio_dec) = session.audio_decoder.as_mut() else {
        return;
    };
    while let Some(pcm) = audio_dec.receive_pcm() {
        session
            .audio_output
            .push_pcm_f32(&pcm.samples, pcm.sample_rate, pcm.channels);
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
        if !feed_next_packet(session, selection) {
            break;
        }
    }
}

fn feed_next_packet(session: &mut ActiveSession, selection: &TrackSelectionParameters) -> bool {
    let Some(packet) = session.demuxer.peek_packet() else {
        return handle_demux_eof(session, selection);
    };
    let stream_index = packet.stream_index;
    let pts = session.segment_base_pts.saturating_add(packet.pts);
    let is_video = session
        .video_info
        .as_ref()
        .is_some_and(|v| v.stream_index == stream_index);
    let is_audio = session
        .active_audio_idx
        .and_then(|idx| session.audio_tracks.get(idx))
        .is_some_and(|a| a.stream_index == stream_index);

    if is_video {
        if let Some(decoder) = session.video_decoder.as_mut() {
            match decoder.submit(AccessUnit {
                data: &packet.data,
                timestamp: pts,
                metadata: u64::from(packet.is_keyframe),
            }) {
                Ok(()) => {}
                Err(DecoderError::Full) => return false,
                Err(err) => log::warn!("linux player: video submit error: {err}"),
            }
        }
        session.demuxer.advance();
        return true;
    }
    if is_audio {
        if session
            .seek_target
            .is_some_and(|target| pts.saturating_add(Duration::from_millis(25)) < target)
        {
            session.demuxer.advance();
            return true;
        }
        if let Some(audio_dec) = session.audio_decoder.as_mut()
            && !audio_dec.send_packet(&packet.data)
        {
            return false;
        }
        session.demuxer.advance();
        return true;
    }
    session.demuxer.advance();
    true
}

fn handle_demux_eof(session: &mut ActiveSession, selection: &TrackSelectionParameters) -> bool {
    if let Some(adaptive) = session.adaptive.as_mut()
        && let Ok(Some(seg)) = adaptive.next_segment(selection)
        && let Ok(mut next_demux) = Demuxer::from_bytes(seg.bytes)
    {
        let _ = next_demux.inspect_tracks();
        session.demuxer = next_demux;
        session.segment_base_pts = seg.start_pts;
        return true;
    }
    session.demux_eos_at = Some(Instant::now());
    if let Some(dec) = session.video_decoder.as_mut() {
        drop(dec.drain());
    }
    if let Some(audio) = session.audio_decoder.as_mut() {
        audio.send_eos();
    }
    false
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
        while let Some(front) = session.pending_video_frames.front() {
            match session.audio_output.frame_action(front.info().timestamp) {
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
