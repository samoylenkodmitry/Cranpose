//! Pure-Rust Media Source Extensions (MSE) and authenticated `fetch()` streaming driver for
//! browser [`Player`](cranpose_services::Player) playback.
//!
//! Parses HLS (`.m3u8`) and MPEG-DASH (`.mpd`) manifests in Rust via `cranpose-services` with
//! zero remote CDN `<script>` injection, zero temporary disk files, and full support for
//! [`MediaItem::headers`](cranpose_services::MediaItem::headers).

use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use cranpose_core::delay;
use cranpose_services::{
    AbrVariant, DashAdaptationSet, DashManifest, HlsMasterPlaylist, HlsMediaPlaylist,
    HlsRenditionType, LoadControl, MediaItem, MediaProperties, PlayerError, PlayerSink, TrackEntry,
    TrackFormat, TrackGroup, TrackSelectionParameters, TrackType, parse_dash_mpd, parse_hls_master,
    parse_hls_media, select_abr_variant,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::HtmlVideoElement;
use web_time::Instant;

use super::{
    player::{
        is_generation_alive, record_object_url, set_base_tracks, take_mse_seek,
        track_selection_snapshot,
    },
    player_fetch::{WebManifestKind, fetch_bytes, fetch_text},
};
use crate::player_control::choose_dash_representation;

const DEFAULT_BANDWIDTH_BPS: u32 = 12_000_000;
const BUFFER_POLL_STEP: Duration = Duration::from_millis(200);
const MIN_LIVE_REFRESH: Duration = Duration::from_secs(1);

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "MediaSource")]
    type JsMediaSource;

    #[wasm_bindgen(catch, constructor, js_class = "MediaSource")]
    fn new() -> Result<JsMediaSource, JsValue>;

    #[wasm_bindgen(static_method_of = JsMediaSource, js_name = "isTypeSupported")]
    fn is_type_supported(mime: &str) -> bool;

    #[wasm_bindgen(method, getter, js_class = "MediaSource", js_name = "readyState")]
    fn ready_state(this: &JsMediaSource) -> String;

    #[wasm_bindgen(method, setter, js_class = "MediaSource")]
    fn set_duration(this: &JsMediaSource, value: f64);

    #[wasm_bindgen(catch, method, js_class = "MediaSource", js_name = "addSourceBuffer")]
    fn add_source_buffer(this: &JsMediaSource, mime: &str) -> Result<JsSourceBuffer, JsValue>;

    #[wasm_bindgen(catch, method, js_class = "MediaSource", js_name = "endOfStream")]
    fn end_of_stream(this: &JsMediaSource) -> Result<(), JsValue>;

    #[wasm_bindgen(method, js_class = "MediaSource", js_name = "addEventListener")]
    fn add_event_listener(this: &JsMediaSource, kind: &str, listener: &js_sys::Function);

    #[wasm_bindgen(method, js_class = "MediaSource", js_name = "removeEventListener")]
    fn remove_event_listener(this: &JsMediaSource, kind: &str, listener: &js_sys::Function);

    #[wasm_bindgen(js_name = "SourceBuffer")]
    type JsSourceBuffer;

    #[wasm_bindgen(method, getter, js_class = "SourceBuffer")]
    fn updating(this: &JsSourceBuffer) -> bool;

    #[wasm_bindgen(catch, method, js_class = "SourceBuffer", js_name = "appendBuffer")]
    fn append_buffer(this: &JsSourceBuffer, data: &js_sys::Uint8Array) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, method, js_class = "SourceBuffer")]
    fn abort(this: &JsSourceBuffer) -> Result<(), JsValue>;

    #[wasm_bindgen(method, js_class = "SourceBuffer", js_name = "addEventListener")]
    fn add_event_listener(this: &JsSourceBuffer, kind: &str, listener: &js_sys::Function);

    #[wasm_bindgen(method, js_class = "SourceBuffer", js_name = "removeEventListener")]
    fn remove_event_listener(this: &JsSourceBuffer, kind: &str, listener: &js_sys::Function);

    #[wasm_bindgen(js_namespace = URL, catch, js_name = "createObjectURL")]
    fn create_object_url_for_value(obj: &JsValue) -> Result<String, JsValue>;
}

pub(crate) fn media_source_supported() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("MediaSource"))
        .ok()
        .is_some_and(|v| v.is_function())
}

#[derive(Clone)]
struct MseSegment {
    sequence: u64,
    uri: String,
    init_uri: Option<String>,
    byte_range: Option<(u64, u64)>,
    start_time: Duration,
    duration: Duration,
}

struct MseTrackPlan {
    mime: String,
    segments: Vec<MseSegment>,
    is_live: bool,
    total_duration: Option<Duration>,
    live_playlist_url: Option<String>,
    target_duration: Duration,
}

struct MseOpenPlan {
    video: MseTrackPlan,
    audio: Option<MseTrackPlan>,
    tracks: Vec<TrackGroup>,
}

pub(crate) async fn run_mse_stream(
    id: u64,
    generation: u64,
    video: HtmlVideoElement,
    item: MediaItem,
    kind: WebManifestKind,
    load_control: LoadControl,
    sink: PlayerSink,
) {
    let result = drive_mse_stream(id, generation, &video, &item, kind, load_control, &sink).await;
    if let Err(err) = result
        && is_generation_alive(id, generation)
    {
        sink.set_loading(false);
        sink.report_error(err);
    }
}

struct MseSessionContext<'a> {
    id: u64,
    generation: u64,
    video: &'a HtmlVideoElement,
    headers: &'a [(String, String)],
    load_control: LoadControl,
}

async fn drive_mse_stream(
    id: u64,
    generation: u64,
    video: &HtmlVideoElement,
    item: &MediaItem,
    kind: WebManifestKind,
    load_control: LoadControl,
    sink: &PlayerSink,
) -> Result<(), PlayerError> {
    if !media_source_supported() {
        return Err(PlayerError::Unsupported(
            "MediaSource Extensions unavailable in this browser".into(),
        ));
    }
    let selection = track_selection_snapshot(id);
    let mut plan = resolve_manifest_plan(item, kind, &selection).await?;
    if !is_generation_alive(id, generation) {
        return Ok(());
    }
    set_base_tracks(id, std::mem::take(&mut plan.tracks));
    sink.set_media_properties(MediaProperties {
        duration: plan.video.total_duration,
        is_live: plan.video.is_live,
        is_seekable: !plan.video.is_live && plan.video.total_duration.is_some(),
    });

    let media_source = open_media_source(id, generation, video, &plan.video).await?;
    if !is_generation_alive(id, generation) {
        return Ok(());
    }
    let video_sb = add_supported_buffer(&media_source, &plan.video.mime, TrackType::Video)?;
    let audio_sb = match plan.audio.as_ref() {
        Some(audio) => match add_supported_buffer(&media_source, &audio.mime, TrackType::Audio) {
            Ok(sb) => Some(sb),
            Err(err) => {
                log::debug!("cranpose web MSE: skipping unsupported audio track: {err}");
                plan.audio = None;
                None
            }
        },
        None => None,
    };
    let ctx = MseSessionContext {
        id,
        generation,
        video,
        headers: &item.headers,
        load_control,
    };
    pump_mse_loop(&ctx, &media_source, &video_sb, audio_sb.as_ref(), &mut plan).await
}

async fn open_media_source(
    id: u64,
    generation: u64,
    video: &HtmlVideoElement,
    video_plan: &MseTrackPlan,
) -> Result<JsMediaSource, PlayerError> {
    let media_source = JsMediaSource::new()
        .map_err(|err| PlayerError::Source(format!("MediaSource::new: {err:?}")))?;
    let url = create_object_url_for_value(media_source.as_ref())
        .map_err(|err| PlayerError::Source(format!("MediaSource object URL: {err:?}")))?;
    record_object_url(id, url.clone());
    let open_future = SourceOpenFuture::new(&media_source);
    video.set_src(&url);
    video.load();
    open_future.await?;
    if is_generation_alive(id, generation)
        && let Some(duration) = video_plan.total_duration
        && !video_plan.is_live
        && duration > Duration::ZERO
    {
        media_source.set_duration(duration.as_secs_f64());
    }
    Ok(media_source)
}

async fn pump_mse_loop(
    ctx: &MseSessionContext<'_>,
    media_source: &JsMediaSource,
    video_sb: &JsSourceBuffer,
    audio_sb: Option<&JsSourceBuffer>,
    plan: &mut MseOpenPlan,
) -> Result<(), PlayerError> {
    let mut video_cursor = BufferCursor::default();
    let mut audio_cursor = BufferCursor::default();
    let mut bandwidth_bps = DEFAULT_BANDWIDTH_BPS;
    let mut ended = false;

    while is_generation_alive(ctx.id, ctx.generation) {
        if let Some(target) = take_mse_seek(ctx.id) {
            ended = false;
            apply_mse_seek(
                video_sb,
                audio_sb,
                plan,
                &mut video_cursor,
                &mut audio_cursor,
                target,
            );
        }
        if ended || buffer_ahead(ctx.video) >= ctx.load_control.max_buffer {
            delay(BUFFER_POLL_STEP).await;
            continue;
        }
        if append_next_segments(
            ctx,
            video_sb,
            audio_sb,
            plan,
            &mut video_cursor,
            &mut audio_cursor,
            &mut bandwidth_bps,
        )
        .await?
        {
            continue;
        }
        if plan.video.is_live && plan.video.live_playlist_url.is_some() {
            let wait = (plan.video.target_duration / 2).max(MIN_LIVE_REFRESH);
            delay(wait).await;
            if is_generation_alive(ctx.id, ctx.generation) {
                plan.video.refresh_live_hls(ctx.headers).await?;
            }
            continue;
        }
        finish_media_source(media_source, video_sb, audio_sb);
        ended = true;
        delay(BUFFER_POLL_STEP).await;
    }
    Ok(())
}

fn apply_mse_seek(
    video_sb: &JsSourceBuffer,
    audio_sb: Option<&JsSourceBuffer>,
    plan: &MseOpenPlan,
    video_cursor: &mut BufferCursor,
    audio_cursor: &mut BufferCursor,
    target: Duration,
) {
    let _ = video_sb.abort();
    if let Some(sb) = audio_sb {
        let _ = sb.abort();
    }
    video_cursor.seek_to(&plan.video.segments, target);
    if let Some(audio) = &plan.audio {
        audio_cursor.seek_to(&audio.segments, target);
    }
}

async fn append_next_segments(
    ctx: &MseSessionContext<'_>,
    video_sb: &JsSourceBuffer,
    audio_sb: Option<&JsSourceBuffer>,
    plan: &MseOpenPlan,
    video_cursor: &mut BufferCursor,
    audio_cursor: &mut BufferCursor,
    bandwidth_bps: &mut u32,
) -> Result<bool, PlayerError> {
    let mut appended = false;
    if let Some(seg) = plan.video.segments.get(video_cursor.next_index) {
        append_segment(video_sb, seg, ctx, video_cursor, bandwidth_bps).await?;
        appended = true;
    }
    if let (Some(audio), Some(sb)) = (&plan.audio, audio_sb)
        && let Some(seg) = audio.segments.get(audio_cursor.next_index)
    {
        append_segment(sb, seg, ctx, audio_cursor, bandwidth_bps).await?;
        appended = true;
    }
    Ok(appended)
}

fn finish_media_source(
    media_source: &JsMediaSource,
    video_sb: &JsSourceBuffer,
    audio_sb: Option<&JsSourceBuffer>,
) {
    if media_source.ready_state() == "open"
        && !video_sb.updating()
        && !audio_sb.is_some_and(JsSourceBuffer::updating)
    {
        let _ = media_source.end_of_stream();
    }
}

#[derive(Default)]
struct BufferCursor {
    next_index: usize,
    loaded_init: Option<String>,
}

impl BufferCursor {
    fn seek_to(&mut self, segments: &[MseSegment], target: Duration) {
        self.next_index = segments
            .iter()
            .position(|seg| seg.start_time.saturating_add(seg.duration) > target)
            .unwrap_or_else(|| segments.len().saturating_sub(1));
    }
}

async fn append_segment(
    sb: &JsSourceBuffer,
    seg: &MseSegment,
    ctx: &MseSessionContext<'_>,
    cursor: &mut BufferCursor,
    bandwidth_bps: &mut u32,
) -> Result<(), PlayerError> {
    if let Some(init_uri) = seg.init_uri.as_deref()
        && cursor.loaded_init.as_deref() != Some(init_uri)
    {
        let init_bytes = fetch_bytes(init_uri, ctx.headers, None)
            .await
            .map_err(PlayerError::Network)?;
        if !is_generation_alive(ctx.id, ctx.generation) {
            return Ok(());
        }
        append_bytes_and_wait(sb, &init_bytes).await?;
        cursor.loaded_init = Some(init_uri.to_owned());
    }
    let started = Instant::now();
    let bytes = fetch_bytes(&seg.uri, ctx.headers, seg.byte_range)
        .await
        .map_err(PlayerError::Network)?;
    if !is_generation_alive(ctx.id, ctx.generation) {
        return Ok(());
    }
    update_bandwidth(bandwidth_bps, bytes.len(), started.elapsed());
    append_bytes_and_wait(sb, &bytes).await?;
    cursor.next_index += 1;
    Ok(())
}

async fn append_bytes_and_wait(sb: &JsSourceBuffer, bytes: &[u8]) -> Result<(), PlayerError> {
    if bytes.is_empty() {
        return Ok(());
    }
    let update = SourceBufferUpdateFuture::new(sb);
    let array = js_sys::Uint8Array::from(bytes);
    sb.append_buffer(&array)
        .map_err(|err| PlayerError::Decoder(format!("SourceBuffer::appendBuffer: {err:?}")))?;
    update.await
}

fn add_supported_buffer(
    media_source: &JsMediaSource,
    preferred: &str,
    track_type: TrackType,
) -> Result<JsSourceBuffer, PlayerError> {
    let fallbacks: &[&str] = match track_type {
        TrackType::Audio => &[
            "audio/mp4; codecs=\"mp4a.40.2\"",
            "audio/mp4; codecs=\"mp4a.40.5\"",
            "audio/mp4",
            "audio/webm; codecs=\"opus\"",
        ],
        _ => &[
            "video/mp4; codecs=\"avc1.640028, mp4a.40.2\"",
            "video/mp4; codecs=\"avc1.640028\"",
            "video/mp4",
            "video/webm; codecs=\"vp09.00.10.08, opus\"",
        ],
    };
    for mime in std::iter::once(preferred).chain(fallbacks.iter().copied()) {
        if !mime.is_empty() && JsMediaSource::is_type_supported(mime) {
            return media_source
                .add_source_buffer(mime)
                .map_err(|err| PlayerError::Decoder(format!("addSourceBuffer({mime}): {err:?}")));
        }
    }
    Err(PlayerError::Unsupported(format!(
        "browser MSE does not support MIME `{preferred}`"
    )))
}

async fn resolve_manifest_plan(
    item: &MediaItem,
    kind: WebManifestKind,
    selection: &TrackSelectionParameters,
) -> Result<MseOpenPlan, PlayerError> {
    let text = fetch_text(&item.uri, &item.headers)
        .await
        .map_err(PlayerError::Network)?;
    match kind {
        WebManifestKind::Hls => resolve_hls_plan(&item.uri, &text, &item.headers, selection).await,
        WebManifestKind::Dash => resolve_dash_plan(&item.uri, &text, selection),
    }
}

async fn resolve_hls_plan(
    uri: &str,
    text: &str,
    headers: &[(String, String)],
    selection: &TrackSelectionParameters,
) -> Result<MseOpenPlan, PlayerError> {
    let master = parse_hls_master(uri, text)
        .map_err(|err| PlayerError::Source(format!("HLS manifest: {err}")))?;
    if !master.variants.is_empty() {
        let abr: Vec<AbrVariant> = master
            .variants
            .iter()
            .enumerate()
            .map(|(index, v)| AbrVariant {
                index,
                bitrate: v.bandwidth,
                width: v.resolution.map(|r| r.0),
                height: v.resolution.map(|r| r.1),
            })
            .collect();
        let selected = select_abr_variant(&abr, DEFAULT_BANDWIDTH_BPS, selection).unwrap_or(0);
        let variant = &master.variants[selected];
        let media_text = fetch_text(&variant.uri, headers)
            .await
            .map_err(PlayerError::Network)?;
        let media = parse_hls_media(&variant.uri, &media_text)
            .map_err(|err| PlayerError::Source(format!("HLS media playlist: {err}")))?;
        ensure_mse_compatible_hls(&media)?;
        let audio_rendition = find_hls_audio_rendition(&master, variant.audio_group.as_deref());
        let (video_codecs, audio_codecs) = if audio_rendition.is_some() {
            split_av_codecs(variant.codecs.as_deref())
        } else {
            (variant.codecs.clone(), None)
        };
        let audio = match audio_rendition {
            Some(audio_url) => {
                let audio_text = fetch_text(audio_url, headers)
                    .await
                    .map_err(PlayerError::Network)?;
                let audio_media = parse_hls_media(audio_url, &audio_text)
                    .map_err(|err| PlayerError::Source(format!("HLS audio playlist: {err}")))?;
                let audio_mime = build_mse_mime(
                    Some("audio/mp4"),
                    audio_codecs.as_deref().or(Some("mp4a.40.2")),
                );
                Some(plan_from_hls_media(
                    audio_url.to_owned(),
                    audio_mime,
                    &audio_media,
                ))
            }
            None => None,
        };
        let mime = build_mse_mime(None, video_codecs.as_deref());
        let tracks = build_hls_tracks(&master, selected);
        return Ok(MseOpenPlan {
            video: plan_from_hls_media(variant.uri.clone(), mime, &media),
            audio,
            tracks,
        });
    }
    let media = parse_hls_media(uri, text)
        .map_err(|err| PlayerError::Source(format!("HLS media playlist: {err}")))?;
    ensure_mse_compatible_hls(&media)?;
    Ok(MseOpenPlan {
        video: plan_from_hls_media(uri.to_owned(), build_mse_mime(None, None), &media),
        audio: None,
        tracks: Vec::new(),
    })
}

fn ensure_mse_compatible_hls(media: &HlsMediaPlaylist) -> Result<(), PlayerError> {
    let Some(first) = media.segments.first() else {
        return Err(PlayerError::Source(
            "HLS playlist contains no segments".into(),
        ));
    };
    let path = first.uri.split('?').next().unwrap_or(&first.uri);
    if first.init_uri.is_none() && path.to_ascii_lowercase().ends_with(".ts") {
        return Err(PlayerError::Unsupported(
            "HLS MPEG-TS requires native HLS (Safari/desktop/mobile); use fMP4 HLS or MPEG-DASH in this browser".into(),
        ));
    }
    Ok(())
}

fn find_hls_audio_rendition<'a>(
    master: &'a HlsMasterPlaylist,
    group_id: Option<&str>,
) -> Option<&'a str> {
    let group_id = group_id?;
    master
        .renditions
        .iter()
        .find(|r| {
            r.kind == HlsRenditionType::Audio
                && r.group_id == group_id
                && r.is_default
                && r.uri.is_some()
        })
        .or_else(|| {
            master.renditions.iter().find(|r| {
                r.kind == HlsRenditionType::Audio && r.group_id == group_id && r.uri.is_some()
            })
        })
        .and_then(|r| r.uri.as_deref())
}

fn split_av_codecs(codecs: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = codecs else {
        return (None, None);
    };
    let mut video = Vec::new();
    let mut audio = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("mp4a")
            || lower.starts_with("opus")
            || lower.starts_with("ac-3")
            || lower.starts_with("ec-3")
            || lower.starts_with("flac")
            || lower.starts_with("vorbis")
        {
            audio.push(part);
        } else {
            video.push(part);
        }
    }
    let v = (!video.is_empty()).then(|| video.join(","));
    let a = (!audio.is_empty()).then(|| audio.join(","));
    (v, a)
}

fn plan_from_hls_media(url: String, mime: String, media: &HlsMediaPlaylist) -> MseTrackPlan {
    let mut cursor = Duration::ZERO;
    let segments = media
        .segments
        .iter()
        .map(|seg| {
            let start_time = cursor;
            cursor = cursor.saturating_add(seg.duration);
            MseSegment {
                sequence: seg.sequence,
                uri: seg.uri.clone(),
                init_uri: seg.init_uri.clone(),
                byte_range: seg.byte_range,
                start_time,
                duration: seg.duration,
            }
        })
        .collect();
    MseTrackPlan {
        mime,
        segments,
        is_live: media.is_live,
        total_duration: (!media.is_live).then(|| media.total_duration()),
        live_playlist_url: media.is_live.then_some(url),
        target_duration: media.target_duration,
    }
}

impl MseTrackPlan {
    async fn refresh_live_hls(&mut self, headers: &[(String, String)]) -> Result<(), PlayerError> {
        let Some(url) = self.live_playlist_url.as_deref() else {
            return Ok(());
        };
        let text = fetch_text(url, headers)
            .await
            .map_err(PlayerError::Network)?;
        let media = parse_hls_media(url, &text)
            .map_err(|err| PlayerError::Source(format!("live HLS refresh: {err}")))?;
        let last_seq = self.segments.last().map(|s| s.sequence);
        let mut cursor = self
            .segments
            .last()
            .map_or(Duration::ZERO, |s| s.start_time.saturating_add(s.duration));
        for seg in media.segments {
            if last_seq.is_none_or(|seq| seg.sequence > seq) {
                self.segments.push(MseSegment {
                    sequence: seg.sequence,
                    uri: seg.uri,
                    init_uri: seg.init_uri,
                    byte_range: seg.byte_range,
                    start_time: cursor,
                    duration: seg.duration,
                });
                cursor = cursor.saturating_add(seg.duration);
            }
        }
        if !media.is_live {
            self.is_live = false;
        }
        Ok(())
    }
}

fn resolve_dash_plan(
    uri: &str,
    xml: &str,
    selection: &TrackSelectionParameters,
) -> Result<MseOpenPlan, PlayerError> {
    let manifest: DashManifest =
        parse_dash_mpd(uri, xml).map_err(|err| PlayerError::Source(format!("DASH MPD: {err}")))?;
    let video_set = manifest
        .adaptation_sets
        .iter()
        .find(|set| set.track_type == TrackType::Video)
        .ok_or_else(|| PlayerError::Source("DASH MPD has no video AdaptationSet".into()))?;
    let (video_plan, video_group) =
        plan_from_dash_set(video_set, manifest.is_live, manifest.duration, selection)?;
    let mut tracks = vec![video_group];
    let audio = manifest
        .adaptation_sets
        .iter()
        .find(|set| set.track_type == TrackType::Audio)
        .and_then(|set| {
            let (plan, group) =
                plan_from_dash_set(set, manifest.is_live, manifest.duration, selection).ok()?;
            tracks.push(group);
            Some(plan)
        });
    Ok(MseOpenPlan {
        video: video_plan,
        audio,
        tracks,
    })
}

fn plan_from_dash_set(
    set: &DashAdaptationSet,
    is_live: bool,
    manifest_duration: Option<Duration>,
    selection: &TrackSelectionParameters,
) -> Result<(MseTrackPlan, TrackGroup), PlayerError> {
    let supported_flags: Vec<bool> = set
        .representations
        .iter()
        .map(|r| {
            let mime = build_mse_mime(r.mime_type.as_deref(), r.codecs.as_deref());
            JsMediaSource::is_type_supported(&mime)
        })
        .collect();
    let selected = choose_supported_dash_representation(set, &supported_flags, selection);
    let rep = &set.representations[selected];
    if rep.segments.is_empty() {
        return Err(PlayerError::Source(
            "DASH representation contains no playable segments".into(),
        ));
    }
    let mut cursor = Duration::ZERO;
    let segments: Vec<MseSegment> = rep
        .segments
        .iter()
        .enumerate()
        .map(|(idx, seg)| {
            let start_time = cursor;
            cursor = cursor.saturating_add(seg.duration);
            MseSegment {
                sequence: idx as u64,
                uri: seg.uri.clone(),
                init_uri: rep.init_uri.clone(),
                byte_range: None,
                start_time,
                duration: seg.duration,
            }
        })
        .collect();
    let mime = build_mse_mime(rep.mime_type.as_deref(), rep.codecs.as_deref());
    let group = TrackGroup {
        id: if set.track_type == TrackType::Audio {
            "dash-audio".into()
        } else {
            "dash-video".into()
        },
        track_type: set.track_type,
        adaptive_supported: set.representations.len() > 1,
        tracks: set
            .representations
            .iter()
            .enumerate()
            .map(|(idx, r)| TrackEntry {
                format: TrackFormat {
                    id: r.id.clone(),
                    mime_type: r.mime_type.clone(),
                    codecs: r.codecs.clone(),
                    width: r.width,
                    height: r.height,
                    frame_rate: r.frame_rate,
                    bitrate: Some(r.bandwidth),
                    language: set.language.clone(),
                    ..TrackFormat::default()
                },
                supported: supported_flags[idx],
                selected: idx == selected,
            })
            .collect(),
    };
    let total_duration = manifest_duration.or_else(|| (!cursor.is_zero()).then_some(cursor));
    Ok((
        MseTrackPlan {
            mime,
            segments,
            is_live,
            total_duration,
            live_playlist_url: None,
            target_duration: Duration::from_secs(2),
        },
        group,
    ))
}

fn choose_supported_dash_representation(
    set: &DashAdaptationSet,
    supported: &[bool],
    selection: &TrackSelectionParameters,
) -> usize {
    let variants: Vec<AbrVariant> = set
        .representations
        .iter()
        .enumerate()
        .filter(|(idx, r)| supported[*idx] && !r.segments.is_empty())
        .map(|(index, r)| AbrVariant {
            index,
            bitrate: r.bandwidth,
            width: r.width,
            height: r.height,
        })
        .collect();
    if let Some(best) = select_abr_variant(&variants, DEFAULT_BANDWIDTH_BPS, selection) {
        return best;
    }
    choose_dash_representation(set, DEFAULT_BANDWIDTH_BPS, selection)
}

fn build_hls_tracks(master: &HlsMasterPlaylist, selected: usize) -> Vec<TrackGroup> {
    let entries = master
        .variants
        .iter()
        .enumerate()
        .map(|(idx, v)| TrackEntry {
            format: TrackFormat {
                id: format!("hls-{idx}"),
                codecs: v.codecs.clone(),
                width: v.resolution.map(|r| r.0),
                height: v.resolution.map(|r| r.1),
                frame_rate: v.frame_rate,
                bitrate: Some(v.bandwidth),
                ..TrackFormat::default()
            },
            supported: true,
            selected: idx == selected,
        })
        .collect();
    vec![TrackGroup {
        id: "hls-video".into(),
        track_type: TrackType::Video,
        tracks: entries,
        adaptive_supported: master.variants.len() > 1,
    }]
}

fn build_mse_mime(mime_type: Option<&str>, codecs: Option<&str>) -> String {
    let container = mime_type.unwrap_or("video/mp4");
    match codecs.filter(|c| !c.is_empty()) {
        Some(c) => format!("{container}; codecs=\"{c}\""),
        None => format!("{container}; codecs=\"avc1.640028, mp4a.40.2\""),
    }
}

fn buffer_ahead(video: &HtmlVideoElement) -> Duration {
    let current = video.current_time();
    if !current.is_finite() || current < 0.0 {
        return Duration::ZERO;
    }
    let ranges = video.buffered();
    let len = ranges.length();
    for idx in 0..len {
        let (Ok(start), Ok(end)) = (ranges.start(idx), ranges.end(idx)) else {
            continue;
        };
        if current + 0.5 >= start && current <= end + 0.5 {
            return Duration::from_secs_f64((end - current).max(0.0));
        }
    }
    Duration::ZERO
}

fn update_bandwidth(estimate: &mut u32, bytes: usize, elapsed: Duration) {
    let secs = elapsed.as_secs_f64();
    if bytes == 0 || secs <= 0.001 {
        return;
    }
    let sample = ((bytes as f64 * 8.0) / secs).clamp(100_000.0, 200_000_000.0) as u32;
    *estimate = (((u64::from(*estimate) * 7) + (u64::from(sample) * 3)) / 10) as u32;
}

struct EventSignal {
    outcome: Option<Result<(), PlayerError>>,
    waker: Option<Waker>,
}

impl EventSignal {
    fn resolve(&mut self, outcome: Result<(), PlayerError>) {
        if self.outcome.is_none() {
            self.outcome = Some(outcome);
            if let Some(waker) = self.waker.take() {
                waker.wake();
            }
        }
    }

    fn poll_signal(cell: &RefCell<Self>, cx: &mut Context<'_>) -> Poll<Result<(), PlayerError>> {
        let mut state = cell.borrow_mut();
        if let Some(outcome) = state.outcome.take() {
            return Poll::Ready(outcome);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

struct SourceOpenFuture<'a> {
    source: &'a JsMediaSource,
    state: Rc<RefCell<EventSignal>>,
    on_open: Closure<dyn FnMut()>,
}

impl<'a> SourceOpenFuture<'a> {
    fn new(source: &'a JsMediaSource) -> Self {
        let state = Rc::new(RefCell::new(EventSignal {
            outcome: (source.ready_state() == "open").then_some(Ok(())),
            waker: None,
        }));
        let cb_state = Rc::clone(&state);
        let on_open = Closure::wrap(Box::new(move || {
            cb_state.borrow_mut().resolve(Ok(()));
        }) as Box<dyn FnMut()>);
        source.add_event_listener("sourceopen", on_open.as_ref().unchecked_ref());
        Self {
            source,
            state,
            on_open,
        }
    }
}

impl Future for SourceOpenFuture<'_> {
    type Output = Result<(), PlayerError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        EventSignal::poll_signal(&self.state, cx)
    }
}

impl Drop for SourceOpenFuture<'_> {
    fn drop(&mut self) {
        self.source
            .remove_event_listener("sourceopen", self.on_open.as_ref().unchecked_ref());
    }
}

struct SourceBufferUpdateFuture<'a> {
    sb: &'a JsSourceBuffer,
    state: Rc<RefCell<EventSignal>>,
    on_end: Closure<dyn FnMut()>,
    on_error: Closure<dyn FnMut()>,
}

impl<'a> SourceBufferUpdateFuture<'a> {
    fn new(sb: &'a JsSourceBuffer) -> Self {
        let state = Rc::new(RefCell::new(EventSignal {
            outcome: None,
            waker: None,
        }));
        let end_state = Rc::clone(&state);
        let on_end = Closure::wrap(Box::new(move || {
            end_state.borrow_mut().resolve(Ok(()));
        }) as Box<dyn FnMut()>);
        let err_state = Rc::clone(&state);
        let on_error = Closure::wrap(Box::new(move || {
            err_state.borrow_mut().resolve(Err(PlayerError::Decoder(
                "SourceBuffer update error".into(),
            )));
        }) as Box<dyn FnMut()>);
        sb.add_event_listener("updateend", on_end.as_ref().unchecked_ref());
        sb.add_event_listener("error", on_error.as_ref().unchecked_ref());
        Self {
            sb,
            state,
            on_end,
            on_error,
        }
    }
}

impl Future for SourceBufferUpdateFuture<'_> {
    type Output = Result<(), PlayerError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        EventSignal::poll_signal(&self.state, cx)
    }
}

impl Drop for SourceBufferUpdateFuture<'_> {
    fn drop(&mut self) {
        self.sb
            .remove_event_listener("updateend", self.on_end.as_ref().unchecked_ref());
        self.sb
            .remove_event_listener("error", self.on_error.as_ref().unchecked_ref());
    }
}
