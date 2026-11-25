//! Zero-copy browser [`PlayerBackend`] backed by a detached `HTMLVideoElement`,
//! `requestVideoFrameCallback`, and pure-Rust MSE for HLS and DASH streams.
//!
//! Every decoded frame is wrapped with [`VideoFrame::from_html_video_element`] and copied straight
//! on the GPU through [`WebVideoConverter`](super::video_converter::WebVideoConverter) on both
//! WebGPU and WebGL2 with zero Canvas2D `getImageData` readback and zero polling while paused.

use std::{sync::Arc, time::Duration};

use cranpose_services::{
    AudioRole, CueGroup, DecodedFrames, FrameInfo, FrameProducer, LoadControl, MediaItem,
    MediaProperties, PlaybackParameters, PlayerBackend, PlayerBackendFactory, PlayerCapabilities,
    PlayerError, PlayerSink, PlayerState, SubtitleConfiguration, SubtitleTrack, TrackEntry,
    TrackFormat, TrackGroup, TrackSelectionParameters, TrackType, Tracks, VideoCodec,
    VideoColorInfo, VideoContainer, VideoDecoder, VideoFrame, VideoRotation, VideoSize,
    parse_subtitles, set_platform_player_factory,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::HtmlVideoElement;

use super::{
    media::{buffered_end, finite_seconds, seconds},
    player_fetch::{
        WebManifestKind, classify_manifest, create_bytes_blob_url, fetch_blob_url, fetch_text,
    },
    player_mse::{media_source_supported, run_mse_stream},
};
use crate::player_control::select_track_index;

const FRAME_QUEUE_DEPTH: usize = 4;

type DomListener = (&'static str, Closure<dyn FnMut()>);
type EventHandler = (&'static str, fn(u64));

struct LoadedWebSubtitle {
    config: SubtitleConfiguration,
    track: SubtitleTrack,
}

pub(crate) struct Session {
    video: HtmlVideoElement,
    sink: PlayerSink,
    load_control: LoadControl,
    producer: Option<FrameProducer>,
    generation: u64,
    first_frame_sent: bool,
    frame_seq: u64,
    want_play: bool,
    active_mse: bool,
    pending_seek: Option<Duration>,
    mse_seek: Option<Duration>,
    volume: f32,
    parameters: PlaybackParameters,
    track_selection: TrackSelectionParameters,
    object_urls: Vec<String>,
    base_tracks: Vec<TrackGroup>,
    subtitles: Vec<LoadedWebSubtitle>,
    last_cues: CueGroup,
    vfc_handle: Option<u32>,
    vfc_closure: Option<Closure<dyn FnMut(f64, JsValue)>>,
    listeners: Vec<DomListener>,
}

struct WebPlayerFactory;

struct WebPlayerBackend {
    id: u64,
}

pub(crate) fn register() {
    set_platform_player_factory(Arc::new(WebPlayerFactory));
}

impl PlayerBackendFactory for WebPlayerFactory {
    fn create(
        &self,
        sink: PlayerSink,
        load_control: LoadControl,
        audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        let video = create_video_element()?;
        if audio_role == AudioRole::Muted {
            video.set_muted(true);
            video.set_volume(0.0);
        }
        let id = crate::thread_state::with(|state| {
            let next = state.web.next_video_player_id.get();
            state
                .web
                .next_video_player_id
                .set(next.saturating_add(1).max(1));
            next
        });
        let listeners = attach_video_listeners(id, &video);
        let session = Session {
            video,
            sink,
            load_control,
            producer: None,
            generation: 0,
            first_frame_sent: false,
            frame_seq: 0,
            want_play: false,
            active_mse: false,
            pending_seek: None,
            mse_seek: None,
            volume: if audio_role == AudioRole::Muted {
                0.0
            } else {
                1.0
            },
            parameters: PlaybackParameters::DEFAULT,
            track_selection: TrackSelectionParameters::default(),
            object_urls: Vec::new(),
            base_tracks: Vec::new(),
            subtitles: Vec::new(),
            last_cues: CueGroup::EMPTY,
            vfc_handle: None,
            vfc_closure: None,
            listeners,
        };
        crate::thread_state::with(|state| {
            state.web.video_players.borrow_mut().insert(id, session);
        });
        Ok(Arc::new(WebPlayerBackend { id }))
    }

    fn capabilities(&self) -> PlayerCapabilities {
        probe_web_capabilities()
    }
}

impl Drop for WebPlayerBackend {
    fn drop(&mut self) {
        self.release();
    }
}

impl PlayerBackend for WebPlayerBackend {
    fn open(&self, item: &MediaItem, start_position: Duration) {
        let id = self.id;
        let item = item.clone();
        let opened = with_session(id, |session| {
            reset_session_media(session);
            session.generation = session.generation.saturating_add(1);
            session.pending_seek = (!start_position.is_zero()).then_some(start_position);
            let (producer, frames) = DecodedFrames::channel(FRAME_QUEUE_DEPTH);
            session.producer = Some(producer);
            (
                session.generation,
                session.video.clone(),
                session.sink.clone(),
                session.load_control,
                frames,
            )
        });
        let Some((generation, video, sink, load_control, frames)) = opened else {
            return;
        };
        sink.set_decoded_frames(Some(frames));
        sink.set_playback_state(PlayerState::Buffering);
        sink.set_loading(true);
        sink.set_cues(CueGroup::EMPTY);

        if !item.subtitle_configurations.is_empty() {
            let subs = item.subtitle_configurations.clone();
            let headers = item.headers.clone();
            spawn_local(async move {
                load_sidecar_subtitles(id, generation, subs, headers).await;
            });
        }

        if let Some(bytes) = item.bytes.as_deref() {
            let mime = item.mime_type.as_deref().unwrap_or("video/mp4");
            match create_bytes_blob_url(bytes, mime) {
                Ok(url) => {
                    record_object_url(id, url.clone());
                    video.set_src(&url);
                    video.load();
                }
                Err(msg) => {
                    sink.set_loading(false);
                    sink.report_error(PlayerError::Source(msg));
                }
            }
            return;
        }

        if let Some(kind) = classify_manifest(&item) {
            let native_hls = kind == WebManifestKind::Hls
                && item.headers.is_empty()
                && !video
                    .can_play_type("application/vnd.apple.mpegurl")
                    .is_empty();
            if !native_hls {
                with_session(id, |session| session.active_mse = true);
                spawn_local(async move {
                    run_mse_stream(id, generation, video, item, kind, load_control, sink).await;
                });
                return;
            }
        }

        if !item.headers.is_empty() {
            let uri = item.uri;
            let headers = item.headers;
            spawn_local(async move {
                match fetch_blob_url(&uri, &headers).await {
                    Ok(url) => {
                        if !is_generation_alive(id, generation) {
                            let _ = web_sys::Url::revoke_object_url(&url);
                            return;
                        }
                        record_object_url(id, url.clone());
                        video.set_src(&url);
                        video.load();
                    }
                    Err(msg) => {
                        if is_generation_alive(id, generation) {
                            sink.set_loading(false);
                            sink.report_error(PlayerError::Network(msg));
                        }
                    }
                }
            });
            return;
        }

        video.set_src(&item.uri);
        video.load();
    }

    fn play(&self) {
        let id = self.id;
        let promise = with_session(id, |session| {
            session.want_play = true;
            apply_audio_settings(&session.video, session.volume, session.parameters);
            session.video.play().ok()
        })
        .flatten();
        schedule_video_frame_callback(id);
        if let Some(promise) = promise {
            spawn_local(async move {
                if let Err(err) = JsFuture::from(promise).await {
                    log::debug!("cranpose web player: browser refused play(): {err:?}");
                }
            });
        }
    }

    fn pause(&self) {
        with_session(self.id, |session| {
            session.want_play = false;
            cancel_vfc(session);
            let _ = session.video.pause();
        });
    }

    fn stop(&self) {
        let sink = with_session(self.id, |session| {
            session.want_play = false;
            session.generation = session.generation.saturating_add(1);
            reset_session_media(session);
            session.sink.clone()
        });
        if let Some(sink) = sink {
            sink.set_decoded_frames(None);
            sink.set_loading(false);
            sink.set_cues(CueGroup::EMPTY);
            sink.set_playback_state(PlayerState::Idle);
        }
    }

    fn seek_to(&self, position: Duration) {
        let id = self.id;
        let applied = with_session(id, |session| {
            if session.active_mse {
                session.mse_seek = Some(position);
            }
            if session.video.ready_state() >= 1 {
                session.pending_seek = None;
                session.video.set_current_time(position.as_secs_f64());
                true
            } else {
                session.pending_seek = Some(position);
                false
            }
        });
        if applied == Some(true) {
            sync_progress_and_cues(id);
            schedule_video_frame_callback(id);
        }
    }

    fn set_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        with_session(self.id, |session| {
            session.volume = clamped;
            apply_audio_settings(&session.video, session.volume, session.parameters);
        });
    }

    fn set_playback_parameters(&self, parameters: PlaybackParameters) {
        with_session(self.id, |session| {
            session.parameters = parameters;
            apply_audio_settings(&session.video, session.volume, session.parameters);
        });
    }

    fn set_track_selection(&self, parameters: &TrackSelectionParameters) {
        let id = self.id;
        with_session(id, |session| {
            session.track_selection = parameters.clone();
        });
        publish_tracks(id);
        sync_progress_and_cues(id);
    }

    fn release(&self) {
        let removed = crate::thread_state::with(|state| {
            state.web.video_players.borrow_mut().remove(&self.id)
        });
        let Some(mut session) = removed else {
            return;
        };
        session.generation = session.generation.saturating_add(1);
        reset_session_media(&mut session);
        for (event, listener) in session.listeners.drain(..) {
            let _ = session
                .video
                .remove_event_listener_with_callback(event, listener.as_ref().unchecked_ref());
        }
        session.sink.set_decoded_frames(None);
    }
}

pub(super) fn is_generation_alive(id: u64, generation: u64) -> bool {
    with_session(id, |session| session.generation == generation).unwrap_or(false)
}

pub(super) fn record_object_url(id: u64, url: String) {
    let kept = with_session(id, |session| {
        session.object_urls.push(url.clone());
    });
    if kept.is_none() {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}

pub(super) fn set_base_tracks(id: u64, groups: Vec<TrackGroup>) {
    with_session(id, |session| {
        session.base_tracks = groups;
    });
    publish_tracks(id);
}

pub(super) fn take_mse_seek(id: u64) -> Option<Duration> {
    with_session(id, |session| session.mse_seek.take()).flatten()
}

pub(super) fn track_selection_snapshot(id: u64) -> TrackSelectionParameters {
    with_session(id, |session| session.track_selection.clone()).unwrap_or_default()
}

fn with_session<R>(id: u64, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    crate::thread_state::with(|state| state.web.video_players.borrow_mut().get_mut(&id).map(f))
}

fn create_video_element() -> Result<HtmlVideoElement, PlayerError> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| PlayerError::Unsupported("DOM document unavailable".into()))?;
    let element = document
        .create_element("video")
        .map_err(|err| PlayerError::Unsupported(format!("createElement('video'): {err:?}")))?;
    let video: HtmlVideoElement = element
        .dyn_into()
        .map_err(|_| PlayerError::Unsupported("created element is not HTMLVideoElement".into()))?;
    video.set_preload("auto");
    video.set_cross_origin(Some("anonymous"));
    let _ = video.set_attribute("playsinline", "");
    let _ = video.set_attribute("webkit-playsinline", "");
    Ok(video)
}

fn apply_audio_settings(video: &HtmlVideoElement, volume: f32, parameters: PlaybackParameters) {
    let clamped = volume.clamp(0.0, 1.0);
    video.set_volume(f64::from(clamped));
    video.set_muted(clamped <= 0.0);
    video.set_playback_rate(f64::from(parameters.speed.clamp(0.25, 4.0)));
}

fn reset_session_media(session: &mut Session) {
    cancel_vfc(session);
    let _ = session.video.pause();
    session.video.set_src("");
    let _ = session.video.remove_attribute("src");
    session.video.load();
    for url in session.object_urls.drain(..) {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
    session.producer = None;
    session.first_frame_sent = false;
    session.frame_seq = 0;
    session.active_mse = false;
    session.pending_seek = None;
    session.mse_seek = None;
    session.base_tracks.clear();
    session.subtitles.clear();
    session.last_cues = CueGroup::EMPTY;
}

fn attach_video_listeners(id: u64, video: &HtmlVideoElement) -> Vec<DomListener> {
    let events: [EventHandler; 12] = [
        ("loadedmetadata", on_loaded_metadata),
        ("loadeddata", on_frame_available),
        ("canplay", on_can_play),
        ("playing", on_playing),
        ("pause", on_pause),
        ("waiting", on_waiting),
        ("ended", on_ended),
        ("seeked", on_frame_available),
        ("timeupdate", on_time_update),
        ("progress", sync_progress_and_cues),
        ("durationchange", on_duration_change),
        ("error", on_error),
    ];
    let mut listeners = Vec::with_capacity(events.len());
    for (name, handler) in events {
        let closure = Closure::wrap(Box::new(move || handler(id)) as Box<dyn FnMut()>);
        let _ = video.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
        listeners.push((name, closure));
    }
    listeners
}

fn on_loaded_metadata(id: u64) {
    let info = with_session(id, |session| {
        let w = session.video.video_width();
        let h = session.video.video_height();
        let raw_duration = session.video.duration();
        let is_live = raw_duration.is_infinite();
        let duration = finite_seconds(raw_duration);
        if let Some(target) = session.pending_seek.take() {
            session.video.set_current_time(target.as_secs_f64());
        }
        if session.base_tracks.is_empty() && w > 0 && h > 0 {
            session.base_tracks.push(TrackGroup {
                id: "web-video".into(),
                track_type: TrackType::Video,
                adaptive_supported: false,
                tracks: vec![TrackEntry {
                    format: TrackFormat {
                        id: "video-0".into(),
                        width: Some(w),
                        height: Some(h),
                        ..TrackFormat::default()
                    },
                    supported: true,
                    selected: true,
                }],
            });
        }
        let should_play = session.want_play && session.video.paused();
        let play_promise = if should_play {
            apply_audio_settings(&session.video, session.volume, session.parameters);
            session.video.play().ok()
        } else {
            None
        };
        (
            session.sink.clone(),
            VideoSize::new(w, h),
            MediaProperties {
                duration,
                is_live,
                is_seekable: !is_live && duration.is_some(),
            },
            play_promise,
        )
    });
    let Some((sink, size, props, play_promise)) = info else {
        return;
    };
    if size.width > 0 && size.height > 0 {
        sink.set_video_size(size);
    }
    sink.set_media_properties(props);
    publish_tracks(id);
    sync_progress_and_cues(id);
    if let Some(promise) = play_promise {
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

fn on_duration_change(id: u64) {
    let update = with_session(id, |session| {
        let raw = session.video.duration();
        let is_live = raw.is_infinite();
        let duration = finite_seconds(raw);
        (
            session.sink.clone(),
            MediaProperties {
                duration,
                is_live,
                is_seekable: !is_live && duration.is_some(),
            },
        )
    });
    if let Some((sink, props)) = update {
        sink.set_media_properties(props);
    }
}

fn on_can_play(id: u64) {
    let sink = with_session(id, |session| session.sink.clone());
    if let Some(sink) = sink {
        sink.set_loading(false);
        sink.set_playback_state(PlayerState::Ready);
    }
    push_current_video_frame(id);
}

fn on_playing(id: u64) {
    let sink = with_session(id, |session| session.sink.clone());
    if let Some(sink) = sink {
        sink.set_loading(false);
        sink.set_playback_state(PlayerState::Ready);
    }
    push_current_video_frame(id);
    schedule_video_frame_callback(id);
}

fn on_pause(id: u64) {
    with_session(id, cancel_vfc);
    sync_progress_and_cues(id);
}

fn on_waiting(id: u64) {
    let sink = with_session(id, |session| session.sink.clone());
    if let Some(sink) = sink {
        sink.set_loading(true);
        sink.set_playback_state(PlayerState::Buffering);
    }
}

fn on_ended(id: u64) {
    let sink = with_session(id, |session| {
        cancel_vfc(session);
        session.sink.clone()
    });
    sync_progress_and_cues(id);
    if let Some(sink) = sink {
        sink.set_loading(false);
        sink.set_playback_state(PlayerState::Ended);
    }
}

fn on_frame_available(id: u64) {
    push_current_video_frame(id);
    sync_progress_and_cues(id);
}

fn on_time_update(id: u64) {
    sync_progress_and_cues(id);
    let needs_fallback_frame = with_session(id, |session| {
        session.vfc_handle.is_none() && !session.video.paused() && !session.video.ended()
    })
    .unwrap_or(false);
    if needs_fallback_frame {
        push_current_video_frame(id);
    }
}

fn on_error(id: u64) {
    let info = with_session(id, |session| {
        cancel_vfc(session);
        let err = session.video.error();
        let (code, message) = err.map_or((0, String::new()), |e| (e.code(), e.message()));
        (session.sink.clone(), code, message)
    });
    let Some((sink, code, message)) = info else {
        return;
    };
    sink.set_loading(false);
    let detail = if message.is_empty() {
        format!("HTMLVideoElement media error code {code}")
    } else {
        format!("HTMLVideoElement error {code}: {message}")
    };
    let error = match code {
        2 => PlayerError::Network(detail),
        3 => PlayerError::Decoder(detail),
        4 => PlayerError::Unsupported(detail),
        _ => PlayerError::Source(detail),
    };
    sink.report_error(error);
}

fn push_current_video_frame(id: u64) {
    let pushed = with_session(id, |session| {
        if session.video.ready_state() < 2 {
            return None;
        }
        let width = session.video.video_width();
        let height = session.video.video_height();
        if width == 0 || height == 0 {
            return None;
        }
        let producer = session.producer.as_ref()?;
        let timestamp = seconds(session.video.current_time());
        session.frame_seq = session.frame_seq.saturating_add(1);
        let frame = VideoFrame::from_html_video_element(
            session.video.clone(),
            FrameInfo {
                timestamp,
                metadata: session.frame_seq,
                decode_time: Duration::ZERO,
            },
            None,
            VideoColorInfo::default(),
            VideoRotation::Degrees0,
        )
        .ok()?;
        producer.push(frame);
        let first = !session.first_frame_sent;
        session.first_frame_sent = true;
        Some((session.sink.clone(), first, VideoSize::new(width, height)))
    })
    .flatten();

    if let Some((sink, first, size)) = pushed
        && first
    {
        sink.set_video_size(size);
        sink.notify_rendered_first_frame();
    }
}

fn schedule_video_frame_callback(id: u64) {
    with_session(id, |session| {
        if session.vfc_handle.is_some() || session.video.paused() || session.video.ended() {
            return;
        }
        let Ok(func) = js_sys::Reflect::get(
            session.video.as_ref(),
            &JsValue::from_str("requestVideoFrameCallback"),
        ) else {
            return;
        };
        let Ok(func) = func.dyn_into::<js_sys::Function>() else {
            return;
        };
        let closure = session.vfc_closure.get_or_insert_with(|| {
            Closure::wrap(Box::new(move |_now: f64, _meta: JsValue| {
                with_session(id, |s| s.vfc_handle = None);
                push_current_video_frame(id);
                sync_progress_and_cues(id);
                schedule_video_frame_callback(id);
            }) as Box<dyn FnMut(f64, JsValue)>)
        });
        if let Ok(handle) = func.call1(session.video.as_ref(), closure.as_ref().unchecked_ref())
            && let Some(number) = handle.as_f64()
        {
            session.vfc_handle = Some(number as u32);
        }
    });
}

fn cancel_vfc(session: &mut Session) {
    let Some(handle) = session.vfc_handle.take() else {
        return;
    };
    if let Ok(func) = js_sys::Reflect::get(
        session.video.as_ref(),
        &JsValue::from_str("cancelVideoFrameCallback"),
    ) && let Ok(func) = func.dyn_into::<js_sys::Function>()
    {
        let _ = func.call1(
            session.video.as_ref(),
            &JsValue::from_f64(f64::from(handle)),
        );
    }
}

fn sync_progress_and_cues(id: u64) {
    let update = with_session(id, |session| {
        let position = seconds(session.video.current_time());
        let buffered = buffered_end(&session.video).unwrap_or(position);
        let selected_sub = select_track_index(
            session.subtitles.len(),
            |idx| session.subtitles[idx].config.language.as_deref(),
            TrackType::Text,
            "subtitles",
            &session.track_selection,
        );
        let cues = selected_sub
            .and_then(|idx| session.subtitles.get(idx))
            .map_or(CueGroup::EMPTY, |sub| sub.track.cues_at(position));
        let cues_changed = cues.cues != session.last_cues.cues;
        if cues_changed {
            session.last_cues = cues.clone();
        }
        (
            session.sink.clone(),
            position,
            buffered,
            cues_changed.then_some(cues),
        )
    });
    if let Some((sink, position, buffered, cues)) = update {
        sink.set_progress(position, buffered);
        if let Some(group) = cues {
            sink.set_cues(group);
        }
    }
}

async fn load_sidecar_subtitles(
    id: u64,
    generation: u64,
    configs: Vec<SubtitleConfiguration>,
    headers: Vec<(String, String)>,
) {
    let mut loaded = Vec::with_capacity(configs.len());
    for config in configs {
        let Ok(text) = fetch_text(&config.uri, &headers).await else {
            continue;
        };
        if !is_generation_alive(id, generation) {
            return;
        }
        let hint = config.mime_type.as_deref().or(Some(config.uri.as_str()));
        let track = parse_subtitles(hint, &text);
        if !track.is_empty() {
            loaded.push(LoadedWebSubtitle { config, track });
        }
    }
    if loaded.is_empty() || !is_generation_alive(id, generation) {
        return;
    }
    with_session(id, |session| {
        if session.generation == generation {
            session.subtitles = loaded;
        }
    });
    publish_tracks(id);
    sync_progress_and_cues(id);
}

fn publish_tracks(id: u64) {
    let snapshot = with_session(id, |session| {
        let mut groups = session.base_tracks.clone();
        if !session.subtitles.is_empty() {
            let selected = select_track_index(
                session.subtitles.len(),
                |idx| session.subtitles[idx].config.language.as_deref(),
                TrackType::Text,
                "subtitles",
                &session.track_selection,
            );
            let entries = session
                .subtitles
                .iter()
                .enumerate()
                .map(|(idx, sub)| TrackEntry {
                    format: TrackFormat {
                        id: format!("sub-{idx}"),
                        mime_type: sub.config.mime_type.clone(),
                        language: sub.config.language.clone(),
                        label: sub.config.label.clone(),
                        ..TrackFormat::default()
                    },
                    supported: true,
                    selected: selected == Some(idx),
                })
                .collect();
            groups.push(TrackGroup {
                id: "subtitles".into(),
                track_type: TrackType::Text,
                tracks: entries,
                adaptive_supported: false,
            });
        }
        (session.sink.clone(), Tracks { groups })
    });
    if let Some((sink, tracks)) = snapshot {
        sink.set_tracks(tracks);
    }
}

fn probe_web_capabilities() -> PlayerCapabilities {
    let Ok(video) = create_video_element() else {
        return PlayerCapabilities::default();
    };
    let mse = media_source_supported();
    let mut containers = Vec::new();
    if !video.can_play_type("video/mp4").is_empty() {
        containers.push(VideoContainer::Mp4);
    }
    if mse {
        containers.push(VideoContainer::FragmentedMp4);
    }
    if !video.can_play_type("video/webm").is_empty() {
        containers.push(VideoContainer::Webm);
    }
    if !video.can_play_type("video/x-matroska").is_empty() {
        containers.push(VideoContainer::Mkv);
    }
    if !video
        .can_play_type("application/vnd.apple.mpegurl")
        .is_empty()
        || mse
    {
        containers.push(VideoContainer::Hls);
    }
    if mse {
        containers.push(VideoContainer::Dash);
    }

    let codec_probes = [
        (VideoCodec::H264, "video/mp4; codecs=\"avc1.640028\""),
        (VideoCodec::Hevc, "video/mp4; codecs=\"hvc1.1.6.L93.B0\""),
        (VideoCodec::Vp9, "video/webm; codecs=\"vp09.00.10.08\""),
        (VideoCodec::Av1, "video/mp4; codecs=\"av01.0.04M.08\""),
    ];
    let mut supported_codecs = Vec::new();
    let mut hardware_codecs = Vec::new();
    for (codec, mime) in codec_probes {
        let can_play = !video.can_play_type(mime).is_empty();
        let webcodecs = VideoDecoder::capabilities(codec).ok();
        if can_play || webcodecs.is_some() {
            supported_codecs.push(codec);
        }
        if webcodecs.is_some_and(|c| c.hardware_accelerated == Some(true)) {
            hardware_codecs.push(codec);
        }
    }

    let hdr_10bit = web_sys::window()
        .and_then(|w| w.match_media("(dynamic-range: high)").ok().flatten())
        .is_some_and(|mql| mql.matches())
        || supported_codecs.contains(&VideoCodec::Hevc)
        || supported_codecs.contains(&VideoCodec::Av1);

    PlayerCapabilities {
        supported_containers: containers,
        supported_video_codecs: supported_codecs,
        hardware_video_codecs: hardware_codecs,
        hdr_10bit,
        zero_copy_gpu: true,
    }
}
