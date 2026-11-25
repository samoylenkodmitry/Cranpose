//! Shared off-UI HTTP range reader, pure-Rust HLS/DASH in-memory segment loader, and sidecar
//! subtitle loader shared by native player backends (Android, Linux, Windows).
//!
//! Never writes temp files to disk and honors [`MediaItem::headers`] on every manifest, segment,
//! and subtitle request.

use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_services::{
    AbrVariant, DashAdaptationSet, DashManifest, HlsMasterPlaylist, HlsMediaPlaylist,
    HttpClientRef, HttpControl, HttpRequest, HttpResponse, MediaItem, SubtitleTrack,
    TrackSelectionParameters, TrackType, default_http_client, parse_dash_mpd, parse_hls_master,
    parse_hls_media, parse_subtitles, select_abr_variant,
};

const FORWARD_SKIP_LIMIT: u64 = 256 * 1024;
const DEFAULT_BANDWIDTH_BPS: u32 = 4_000_000;

pub(crate) fn is_http_uri(uri: &str) -> bool {
    uri.split_once("://").is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    })
}

pub(crate) fn local_path_from_uri(uri: &str) -> Option<PathBuf> {
    if let Some(rest) = uri.strip_prefix("file://") {
        let trimmed = rest.strip_prefix("localhost").unwrap_or(rest);
        return Some(PathBuf::from(percent_decode(trimmed)));
    }
    if uri.contains("://") {
        return None;
    }
    Some(PathBuf::from(uri))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[idx + 1]), hex_val(bytes[idx + 2]))
        {
            out.push((hi << 4) | lo);
            idx += 3;
            continue;
        }
        out.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn detect_manifest_kind(item: &MediaItem) -> Option<ManifestKind> {
    if let Some(mime) = item.mime_type.as_deref() {
        let lower = mime.to_ascii_lowercase();
        if lower.contains("mpegurl") {
            return Some(ManifestKind::Hls);
        }
        if lower.contains("dash+xml") {
            return Some(ManifestKind::Dash);
        }
    }
    let path = item.uri.split('?').next().unwrap_or(&item.uri);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".m3u8") {
        Some(ManifestKind::Hls)
    } else if lower.ends_with(".mpd") {
        Some(ManifestKind::Dash)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManifestKind {
    Hls,
    Dash,
}

#[derive(Clone, Default)]
pub(crate) struct FetchCancel {
    stopped: Arc<AtomicBool>,
    active: Arc<Mutex<Option<HttpControl>>>,
}

impl FetchCancel {
    pub(crate) fn cancel(&self) {
        self.stopped.store(true, Ordering::Release);
        let control = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(control) = control {
            control.cancel();
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn arm(&self, control: HttpControl) {
        *self.active.lock().unwrap_or_else(PoisonError::into_inner) = Some(control);
    }

    fn disarm(&self) {
        let control = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(control) = control {
            control.cancel();
        }
    }
}

pub(crate) fn fetch_http_bytes(
    url: &str,
    headers: &[(String, String)],
    byte_range: Option<(u64, u64)>,
    cancel: &FetchCancel,
) -> Result<Vec<u8>, String> {
    if cancel.is_cancelled() {
        return Err("request cancelled".into());
    }
    let client = default_http_client();
    let mut req = HttpRequest::get(url);
    for (name, value) in headers {
        req = req.header(name.clone(), value.clone());
    }
    if let Some((length, offset)) = byte_range
        && length > 0
    {
        let end = offset.saturating_add(length).saturating_sub(1);
        req = req.header("Range", format!("bytes={offset}-{end}"));
    }
    let control = HttpControl::new();
    cancel.arm(control.clone());
    let response = pollster::block_on(client.send(&req, control))
        .and_then(HttpResponse::error_for_status)
        .map_err(|e| format!("{url}: {e}"))?;
    let mut body = Vec::new();
    if let Some(len) = response.content_length
        && let Ok(cap) = usize::try_from(len)
    {
        body.reserve(cap.min(16 * 1024 * 1024));
    }
    loop {
        if cancel.is_cancelled() {
            cancel.disarm();
            return Err("request cancelled".into());
        }
        match pollster::block_on(response.read_chunk()) {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(error) => {
                cancel.disarm();
                return Err(format!("{url}: {error}"));
            }
        }
    }
    cancel.disarm();
    Ok(body)
}

struct ActiveHttpStream {
    response: HttpResponse,
    next_offset: u64,
    chunk: Vec<u8>,
    cursor: usize,
    ended: bool,
}

impl ActiveHttpStream {
    fn ensure_bytes(&mut self) -> io::Result<usize> {
        while self.cursor >= self.chunk.len() {
            if self.ended {
                return Ok(0);
            }
            match pollster::block_on(self.response.read_chunk()) {
                Ok(Some(data)) => {
                    self.chunk = data;
                    self.cursor = 0;
                }
                Ok(None) => {
                    self.ended = true;
                    return Ok(0);
                }
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        Ok(self.chunk.len() - self.cursor)
    }

    fn take(&mut self, count: usize) -> &[u8] {
        let start = self.cursor;
        self.cursor += count;
        self.next_offset = self.next_offset.saturating_add(count as u64);
        &self.chunk[start..start + count]
    }
}

pub(crate) struct HttpRangeReader {
    client: HttpClientRef,
    url: String,
    headers: Vec<(String, String)>,
    total_len: Option<u64>,
    ranged: bool,
    stream: Option<ActiveHttpStream>,
    cancel: FetchCancel,
}

impl HttpRangeReader {
    pub(crate) fn open(
        url: &str,
        headers: Vec<(String, String)>,
        cancel: FetchCancel,
    ) -> Result<Self, String> {
        let client = default_http_client();
        let (stream, total_len, ranged) =
            open_range(&client, url, &headers, 0, &cancel).map_err(|e| format!("{url}: {e}"))?;
        Ok(Self {
            client,
            url: url.to_owned(),
            headers,
            total_len,
            ranged,
            stream: Some(stream),
            cancel,
        })
    }

    pub(crate) fn size(&self) -> Option<u64> {
        self.total_len
    }

    pub(crate) fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.cancel.is_cancelled() {
            return Ok(0);
        }
        if self.total_len.is_some_and(|len| offset >= len) {
            return Ok(0);
        }
        self.align_to(offset)?;
        let Some(stream) = self.stream.as_mut() else {
            return Ok(0);
        };
        let available = stream.ensure_bytes()?;
        if available == 0 {
            return Ok(0);
        }
        let count = available.min(buf.len());
        buf[..count].copy_from_slice(stream.take(count));
        Ok(count)
    }

    fn align_to(&mut self, target: u64) -> io::Result<()> {
        if let Some(stream) = self.stream.as_mut() {
            if stream.next_offset == target {
                return Ok(());
            }
            if target > stream.next_offset && target - stream.next_offset <= FORWARD_SKIP_LIMIT {
                while stream.next_offset < target {
                    let available = stream.ensure_bytes()?;
                    if available == 0 {
                        break;
                    }
                    let remaining =
                        usize::try_from(target - stream.next_offset).unwrap_or(usize::MAX);
                    let step = remaining.min(available);
                    let _: &[u8] = stream.take(step);
                }
                if stream.next_offset == target {
                    return Ok(());
                }
            }
        }
        self.stream = None;
        self.cancel.disarm();
        if target > 0 && !self.ranged {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "remote server does not support HTTP byte ranges",
            ));
        }
        let (stream, len, _) =
            open_range(&self.client, &self.url, &self.headers, target, &self.cancel)?;
        if self.total_len.is_none() {
            self.total_len = len;
        }
        self.stream = Some(stream);
        Ok(())
    }
}

impl Drop for HttpRangeReader {
    fn drop(&mut self) {
        self.stream = None;
        self.cancel.disarm();
    }
}

fn open_range(
    client: &HttpClientRef,
    url: &str,
    headers: &[(String, String)],
    offset: u64,
    cancel: &FetchCancel,
) -> io::Result<(ActiveHttpStream, Option<u64>, bool)> {
    let mut req = HttpRequest::get(url);
    for (name, value) in headers {
        req = req.header(name.clone(), value.clone());
    }
    if offset > 0 {
        req = req.resume_from(offset);
    }
    let control = HttpControl::new();
    cancel.arm(control.clone());
    let response = pollster::block_on(client.send(&req, control))
        .and_then(HttpResponse::error_for_status)
        .map_err(io::Error::other)?;
    if offset > 0 && !response.resumed {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "server ignored HTTP Range request",
        ));
    }
    let total_len = response
        .content_length
        .map(|rem| offset.saturating_add(rem));
    let ranged = response.resumed
        || response.header("accept-ranges").is_some_and(|v| {
            v.split(',')
                .any(|unit| unit.trim().eq_ignore_ascii_case("bytes"))
        });
    Ok((
        ActiveHttpStream {
            response,
            next_offset: offset,
            chunk: Vec::new(),
            cursor: 0,
            ended: false,
        },
        total_len,
        ranged,
    ))
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedSegment {
    pub(crate) start_pts: Duration,
    pub(crate) bytes: Arc<[u8]>,
}

#[derive(Clone, Debug)]
struct SegmentEntry {
    uri: String,
    init_uri: Option<String>,
    byte_range: Option<(u64, u64)>,
    start_pts: Duration,
    duration: Duration,
}

pub(crate) struct AdaptiveStream {
    kind: ManifestKind,
    headers: Vec<(String, String)>,
    cancel: FetchCancel,
    hls_master: Option<HlsMasterPlaylist>,
    dash_manifest: Option<DashManifest>,
    active_media_url: String,
    active_variant_index: usize,
    is_live: bool,
    total_duration: Option<Duration>,
    segments: Vec<SegmentEntry>,
    next_index: usize,
    cached_init_uri: Option<String>,
    cached_init_bytes: Vec<u8>,
    bandwidth_bps: u32,
}

impl AdaptiveStream {
    pub(crate) fn open(
        item: &MediaItem,
        kind: ManifestKind,
        selection: &TrackSelectionParameters,
        cancel: FetchCancel,
    ) -> Result<Self, String> {
        let manifest_bytes = fetch_http_bytes(&item.uri, &item.headers, None, &cancel)?;
        let text = String::from_utf8(manifest_bytes)
            .map_err(|_| "streaming manifest is not valid UTF-8".to_owned())?;
        let mut stream = Self {
            kind,
            headers: item.headers.clone(),
            cancel,
            hls_master: None,
            dash_manifest: None,
            active_media_url: item.uri.clone(),
            active_variant_index: 0,
            is_live: false,
            total_duration: None,
            segments: Vec::new(),
            next_index: 0,
            cached_init_uri: None,
            cached_init_bytes: Vec::new(),
            bandwidth_bps: DEFAULT_BANDWIDTH_BPS,
        };
        match kind {
            ManifestKind::Hls => stream.init_hls(&item.uri, &text, selection)?,
            ManifestKind::Dash => stream.init_dash(&item.uri, &text, selection)?,
        }
        Ok(stream)
    }

    pub(crate) fn is_live(&self) -> bool {
        self.is_live
    }

    pub(crate) fn duration(&self) -> Option<Duration> {
        self.total_duration
    }

    pub(crate) fn is_seekable(&self) -> bool {
        !self.is_live && !self.segments.is_empty()
    }

    pub(crate) fn update_selection(&mut self, selection: &TrackSelectionParameters) {
        if self.kind == ManifestKind::Hls
            && let Some(master) = self.hls_master.clone()
        {
            let abr = hls_abr_variants(&master);
            if let Some(idx) = select_abr_variant(&abr, self.bandwidth_bps, selection)
                && idx != self.active_variant_index
                && let Some(variant) = master.variants.get(idx)
            {
                let target_pts = self
                    .segments
                    .get(self.next_index)
                    .map_or(Duration::ZERO, |s| s.start_pts);
                if self.load_hls_media(&variant.uri).is_ok() {
                    self.active_variant_index = idx;
                    let _: Duration = self.seek_to(target_pts);
                }
            }
        } else if self.kind == ManifestKind::Dash
            && let Some(manifest) = self.dash_manifest.clone()
        {
            let target_pts = self
                .segments
                .get(self.next_index)
                .map_or(Duration::ZERO, |s| s.start_pts);
            if self.select_dash_representation(&manifest, selection) {
                let _: Duration = self.seek_to(target_pts);
            }
        }
    }

    pub(crate) fn seek_to(&mut self, position: Duration) -> Duration {
        if self.segments.is_empty() {
            self.next_index = 0;
            return Duration::ZERO;
        }
        let mut chosen = self.segments.len().saturating_sub(1);
        for (idx, seg) in self.segments.iter().enumerate() {
            if position < seg.start_pts.saturating_add(seg.duration) {
                chosen = idx;
                break;
            }
        }
        self.next_index = chosen;
        position.saturating_sub(self.segments[chosen].start_pts)
    }

    pub(crate) fn next_segment(
        &mut self,
        selection: &TrackSelectionParameters,
    ) -> Result<Option<ResolvedSegment>, String> {
        if self.next_index >= self.segments.len() {
            if self.is_live && self.kind == ManifestKind::Hls {
                let prev_count = self.segments.len();
                let url = self.active_media_url.clone();
                if self.load_hls_media(&url).is_ok() && self.segments.len() > prev_count {
                    self.next_index = prev_count;
                } else {
                    return Ok(None);
                }
            } else {
                return Ok(None);
            }
        }
        let entry = self.segments[self.next_index].clone();
        self.next_index += 1;

        let init_bytes = if let Some(init_uri) = entry.init_uri.as_deref() {
            if self.cached_init_uri.as_deref() != Some(init_uri) {
                let bytes = fetch_http_bytes(init_uri, &self.headers, None, &self.cancel)?;
                self.cached_init_uri = Some(init_uri.to_owned());
                self.cached_init_bytes = bytes;
            }
            self.cached_init_bytes.as_slice()
        } else {
            &[]
        };

        let started = Instant::now();
        let seg_bytes =
            fetch_http_bytes(&entry.uri, &self.headers, entry.byte_range, &self.cancel)?;
        let elapsed = started.elapsed();
        let bytes: Arc<[u8]> = if init_bytes.is_empty() {
            Arc::from(seg_bytes)
        } else {
            let mut combined = Vec::with_capacity(init_bytes.len() + seg_bytes.len());
            combined.extend_from_slice(init_bytes);
            combined.extend_from_slice(&seg_bytes);
            Arc::from(combined)
        };
        self.record_throughput(bytes.len(), elapsed);
        self.update_selection(selection);
        Ok(Some(ResolvedSegment {
            start_pts: entry.start_pts,
            bytes,
        }))
    }

    fn record_throughput(&mut self, bytes: usize, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        if bytes < 4096 || !secs.is_finite() || secs <= 0.001 {
            return;
        }
        let sample_bps = ((bytes as f64 * 8.0) / secs).clamp(100_000.0, f64::from(u32::MAX));
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to finite 100_000..=u32::MAX; budgeted lint only active under clippy_budget"
        )]
        let sample_u32 = sample_bps as u32;
        self.bandwidth_bps =
            u32::try_from((u64::from(self.bandwidth_bps) * 7 + u64::from(sample_u32) * 3) / 10)
                .unwrap_or(u32::MAX);
    }

    fn init_hls(
        &mut self,
        url: &str,
        text: &str,
        selection: &TrackSelectionParameters,
    ) -> Result<(), String> {
        let master = parse_hls_master(url, text).map_err(|e| e.to_string())?;
        if !master.variants.is_empty() {
            let abr = hls_abr_variants(&master);
            let chosen = select_abr_variant(&abr, self.bandwidth_bps, selection).unwrap_or(0);
            let media_url = master.variants[chosen].uri.clone();
            self.hls_master = Some(master);
            self.active_variant_index = chosen;
            return self.load_hls_media(&media_url);
        }
        let media = parse_hls_media(url, text).map_err(|e| e.to_string())?;
        self.apply_hls_media(url, &media);
        Ok(())
    }

    fn load_hls_media(&mut self, url: &str) -> Result<(), String> {
        let bytes = fetch_http_bytes(url, &self.headers, None, &self.cancel)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| "HLS media playlist is not valid UTF-8".to_owned())?;
        let media = parse_hls_media(url, &text).map_err(|e| e.to_string())?;
        self.apply_hls_media(url, &media);
        Ok(())
    }

    fn apply_hls_media(&mut self, url: &str, media: &HlsMediaPlaylist) {
        url.clone_into(&mut self.active_media_url);
        self.is_live = media.is_live;
        self.total_duration = if media.is_live {
            None
        } else {
            Some(media.total_duration())
        };
        let mut cursor = Duration::ZERO;
        self.segments = media
            .segments
            .iter()
            .map(|seg| {
                let start = cursor;
                cursor = cursor.saturating_add(seg.duration);
                SegmentEntry {
                    uri: seg.uri.clone(),
                    init_uri: seg.init_uri.clone(),
                    byte_range: seg.byte_range,
                    start_pts: start,
                    duration: seg.duration,
                }
            })
            .collect();
        self.next_index = 0;
    }

    fn init_dash(
        &mut self,
        url: &str,
        xml: &str,
        selection: &TrackSelectionParameters,
    ) -> Result<(), String> {
        let manifest = parse_dash_mpd(url, xml).map_err(|e| e.to_string())?;
        self.is_live = manifest.is_live;
        self.total_duration = manifest.duration;
        let selected = self.select_dash_representation(&manifest, selection);
        self.dash_manifest = Some(manifest);
        if !selected {
            return Err("DASH manifest contains no playable video or audio segments".into());
        }
        Ok(())
    }

    fn select_dash_representation(
        &mut self,
        manifest: &DashManifest,
        selection: &TrackSelectionParameters,
    ) -> bool {
        let set: Option<&DashAdaptationSet> = manifest
            .adaptation_sets
            .iter()
            .find(|s| s.track_type == TrackType::Video && !s.representations.is_empty())
            .or_else(|| {
                manifest
                    .adaptation_sets
                    .iter()
                    .find(|s| !s.representations.is_empty())
            });
        let Some(set) = set else {
            return false;
        };
        let abr: Vec<AbrVariant> = set
            .representations
            .iter()
            .enumerate()
            .map(|(index, rep)| AbrVariant {
                index,
                bitrate: rep.bandwidth,
                width: rep.width,
                height: rep.height,
            })
            .collect();
        let chosen = select_abr_variant(&abr, self.bandwidth_bps, selection).unwrap_or(0);
        let Some(rep) = set.representations.get(chosen) else {
            return false;
        };
        let mut cursor = Duration::ZERO;
        self.segments = rep
            .segments
            .iter()
            .map(|seg| {
                let start = cursor;
                cursor = cursor.saturating_add(seg.duration);
                SegmentEntry {
                    uri: seg.uri.clone(),
                    init_uri: rep.init_uri.clone(),
                    byte_range: None,
                    start_pts: start,
                    duration: seg.duration,
                }
            })
            .collect();
        if self.total_duration.is_none() && !self.is_live && cursor > Duration::ZERO {
            self.total_duration = Some(cursor);
        }
        self.active_variant_index = chosen;
        !self.segments.is_empty()
    }
}

fn hls_abr_variants(master: &HlsMasterPlaylist) -> Vec<AbrVariant> {
    master
        .variants
        .iter()
        .enumerate()
        .map(|(index, v)| AbrVariant {
            index,
            bitrate: v.average_bandwidth.unwrap_or(v.bandwidth),
            width: v.resolution.map(|(w, _)| w),
            height: v.resolution.map(|(_, h)| h),
        })
        .collect()
}

pub(crate) struct LoadedSubtitle {
    pub(crate) language: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) track: SubtitleTrack,
}

pub(crate) fn load_sidecar_subtitles(
    item: &MediaItem,
    cancel: &FetchCancel,
) -> Vec<LoadedSubtitle> {
    let mut loaded = Vec::new();
    for config in &item.subtitle_configurations {
        let text = if let Some(path) = local_path_from_uri(&config.uri) {
            fs::read_to_string(path).ok()
        } else if is_http_uri(&config.uri) {
            fetch_http_bytes(&config.uri, &item.headers, None, cancel)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            None
        };
        let Some(text) = text else {
            continue;
        };
        let track = parse_subtitles(config.mime_type.as_deref(), &text);
        if !track.cues().is_empty() {
            loaded.push(LoadedSubtitle {
                language: config.language.clone(),
                label: config.label.clone(),
                track,
            });
        }
    }
    loaded
}
