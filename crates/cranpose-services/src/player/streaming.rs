//! Zero-dependency HLS (`.m3u8`) and DASH (`.mpd`) manifest parsers and ABR variant selection.
//!
//! Works identically across Android, iOS, macOS, Windows, Linux, and Web with no external scripts.

use std::time::Duration;

use super::{TrackFormat, TrackSelectionParameters, TrackType};

/// Error parsing an HLS or DASH streaming manifest.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Input does not begin with a recognized HLS `#EXTM3U` or DASH `<MPD>` root.
    #[error("unrecognized streaming manifest: {0}")]
    InvalidFormat(String),
    /// XML parsing failed while decoding a DASH MPD document.
    #[error("DASH XML error: {0}")]
    Xml(String),
}

/// Type of alternate rendition advertised by `#EXT-X-MEDIA`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HlsRenditionType {
    /// Alternate audio track.
    Audio,
    /// Alternate subtitle or caption track.
    Subtitles,
    /// Alternate video angle.
    Video,
}

/// Alternate rendition entry in an [`HlsMasterPlaylist`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HlsRendition {
    /// Kind of media carried by this rendition.
    pub kind: HlsRenditionType,
    /// Group identifier referenced by [`HlsVariant`].
    pub group_id: String,
    /// Display name.
    pub name: String,
    /// Optional BCP-47 language tag.
    pub language: Option<String>,
    /// Resolved rendition playlist URI, if external.
    pub uri: Option<String>,
    /// Whether the manifest marks this rendition as default.
    pub is_default: bool,
}

/// One adaptive bitrate variant stream in an [`HlsMasterPlaylist`].
#[derive(Clone, Debug, PartialEq)]
pub struct HlsVariant {
    /// Resolved media playlist URI.
    pub uri: String,
    /// Peak bitrate in bits per second (`BANDWIDTH`).
    pub bandwidth: u32,
    /// Optional average bitrate in bits per second (`AVERAGE-BANDWIDTH`).
    pub average_bandwidth: Option<u32>,
    /// Optional video dimensions (`RESOLUTION`).
    pub resolution: Option<(u32, u32)>,
    /// Optional RFC 6381 codec list (`CODECS`).
    pub codecs: Option<String>,
    /// Optional frame rate (`FRAME-RATE`).
    pub frame_rate: Option<f32>,
    /// Optional audio rendition group identifier (`AUDIO`).
    pub audio_group: Option<String>,
    /// Optional subtitles rendition group identifier (`SUBTITLES`).
    pub subtitles_group: Option<String>,
}

/// Parsed HLS multivariant (master) playlist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HlsMasterPlaylist {
    /// Adaptive video/muxed variants.
    pub variants: Vec<HlsVariant>,
    /// Alternate audio, subtitle, and video renditions.
    pub renditions: Vec<HlsRendition>,
}

/// One media segment inside an [`HlsMediaPlaylist`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HlsSegment {
    /// Media sequence number (`#EXT-X-MEDIA-SEQUENCE` + offset).
    pub sequence: u64,
    /// Resolved segment URI.
    pub uri: String,
    /// Segment duration (`#EXTINF`).
    pub duration: Duration,
    /// Optional inclusive byte range `(length, offset)` from `#EXT-X-BYTERANGE`.
    pub byte_range: Option<(u64, u64)>,
    /// Optional fMP4 initialization segment URI (`#EXT-X-MAP`).
    pub init_uri: Option<String>,
    /// Whether a timeline discontinuity precedes this segment.
    pub discontinuity: bool,
}

/// Parsed HLS media playlist (VOD or live).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HlsMediaPlaylist {
    /// Target maximum segment duration (`#EXT-X-TARGETDURATION`).
    pub target_duration: Duration,
    /// Starting media sequence number (`#EXT-X-MEDIA-SEQUENCE`).
    pub media_sequence: u64,
    /// `true` when `#EXT-X-ENDLIST` is absent (live sliding window).
    pub is_live: bool,
    /// Ordered media segments.
    pub segments: Vec<HlsSegment>,
}

impl HlsMediaPlaylist {
    /// Total duration of all listed segments.
    pub fn total_duration(&self) -> Duration {
        self.segments
            .iter()
            .fold(Duration::ZERO, |acc, seg| acc.saturating_add(seg.duration))
    }
}

/// Parses an HLS multivariant (`#EXTM3U`) playlist, resolving relative URIs against `base_url`.
pub fn parse_hls_master(base_url: &str, text: &str) -> Result<HlsMasterPlaylist, ManifestError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with("#EXTM3U") {
        return Err(ManifestError::InvalidFormat(
            "missing #EXTM3U header".into(),
        ));
    }
    let mut playlist = HlsMasterPlaylist::default();
    let mut pending_stream_inf: Option<&str> = None;
    for raw in trimmed.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") {
            if let Some(rendition) = parse_hls_rendition(base_url, attrs) {
                playlist.renditions.push(rendition);
            }
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending_stream_inf = Some(attrs);
        } else if !line.starts_with('#')
            && let Some(attrs) = pending_stream_inf.take()
            && let Some(variant) = parse_hls_variant(base_url, attrs, line)
        {
            playlist.variants.push(variant);
        }
    }
    Ok(playlist)
}

/// Parses an HLS media playlist (`#EXTM3U`), resolving relative segment URIs against `base_url`.
pub fn parse_hls_media(base_url: &str, text: &str) -> Result<HlsMediaPlaylist, ManifestError> {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    if !trimmed.starts_with("#EXTM3U") {
        return Err(ManifestError::InvalidFormat(
            "missing #EXTM3U header".into(),
        ));
    }
    let mut playlist = HlsMediaPlaylist {
        is_live: true,
        ..HlsMediaPlaylist::default()
    };
    let mut next_duration: Option<Duration> = None;
    let mut next_range: Option<(u64, u64)> = None;
    let mut prev_range_end: u64 = 0;
    let mut current_init: Option<String> = None;
    let mut next_discontinuity = false;

    for raw in trimmed.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(val) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
            if let Ok(secs) = val.trim().parse::<u64>() {
                playlist.target_duration = Duration::from_secs(secs);
            }
        } else if let Some(val) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            if let Ok(seq) = val.trim().parse::<u64>() {
                playlist.media_sequence = seq;
            }
        } else if line == "#EXT-X-ENDLIST" {
            playlist.is_live = false;
        } else if line == "#EXT-X-DISCONTINUITY" {
            next_discontinuity = true;
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(uri) = find_attr(attrs, "URI") {
                current_init = Some(resolve_url(base_url, uri));
            }
        } else if let Some(val) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            next_range = parse_byte_range(val.trim(), prev_range_end);
        } else if let Some(val) = line.strip_prefix("#EXTINF:") {
            let secs_str = val.split(',').next().unwrap_or("").trim();
            if let Ok(secs) = secs_str.parse::<f64>()
                && secs.is_finite()
                && secs >= 0.0
            {
                next_duration = Some(Duration::from_secs_f64(secs));
            }
        } else if !line.starts_with('#')
            && let Some(duration) = next_duration.take()
        {
            if let Some((len, off)) = next_range {
                prev_range_end = off.saturating_add(len);
            }
            let seq = playlist
                .media_sequence
                .saturating_add(playlist.segments.len() as u64);
            playlist.segments.push(HlsSegment {
                sequence: seq,
                uri: resolve_url(base_url, line),
                duration,
                byte_range: next_range.take(),
                init_uri: current_init.clone(),
                discontinuity: next_discontinuity,
            });
            next_discontinuity = false;
        }
    }
    Ok(playlist)
}

/// One resolved media segment in a DASH representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashSegment {
    /// Resolved segment URI.
    pub uri: String,
    /// Segment duration.
    pub duration: Duration,
}

/// One representation inside a [`DashAdaptationSet`].
#[derive(Clone, Debug, PartialEq)]
pub struct DashRepresentation {
    /// Representation identifier (`id`).
    pub id: String,
    /// Bitrate in bits per second (`bandwidth`).
    pub bandwidth: u32,
    /// Optional video width in pixels.
    pub width: Option<u32>,
    /// Optional video height in pixels.
    pub height: Option<u32>,
    /// Optional frame rate.
    pub frame_rate: Option<f32>,
    /// Optional codec string.
    pub codecs: Option<String>,
    /// Optional MIME type.
    pub mime_type: Option<String>,
    /// Optional initialization segment URI.
    pub init_uri: Option<String>,
    /// Resolved media segment list (populated for `SegmentList` and `SegmentTimeline`).
    pub segments: Vec<DashSegment>,
}

/// One `AdaptationSet` in a [`DashManifest`].
#[derive(Clone, Debug, PartialEq)]
pub struct DashAdaptationSet {
    /// Track type inferred from `contentType` or `mimeType`.
    pub track_type: TrackType,
    /// Optional BCP-47 language tag.
    pub language: Option<String>,
    /// Representations belonging to this adaptation set.
    pub representations: Vec<DashRepresentation>,
}

/// Parsed MPEG-DASH MPD manifest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DashManifest {
    /// `true` for `type="dynamic"` live manifests, `false` for `static` VOD.
    pub is_live: bool,
    /// Optional total presentation duration (`mediaPresentationDuration`).
    pub duration: Option<Duration>,
    /// Discovered adaptation sets across periods.
    pub adaptation_sets: Vec<DashAdaptationSet>,
}

/// Parses an MPEG-DASH XML MPD document, resolving relative URIs against `base_url`.
pub fn parse_dash_mpd(base_url: &str, xml: &str) -> Result<DashManifest, ManifestError> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| ManifestError::Xml(e.to_string()))?;
    let root = doc.root_element();
    if root.tag_name().name() != "MPD" {
        return Err(ManifestError::InvalidFormat(format!(
            "expected <MPD> root, found <{}>",
            root.tag_name().name()
        )));
    }
    let mpd_base = resolve_base_url(root, base_url);
    let is_live = root.attribute("type") == Some("dynamic");
    let duration = root
        .attribute("mediaPresentationDuration")
        .and_then(parse_iso8601_duration);

    let mut adaptation_sets = Vec::new();
    for period in root
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Period")
    {
        let period_base = resolve_base_url(period, &mpd_base);
        for set in period
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "AdaptationSet")
        {
            if let Some(parsed) = parse_dash_adaptation_set(&period_base, set) {
                adaptation_sets.push(parsed);
            }
        }
    }
    Ok(DashManifest {
        is_live,
        duration,
        adaptation_sets,
    })
}

/// Unified adaptive bitrate variant descriptor shared by HLS and DASH selectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbrVariant {
    /// Index of this variant in the caller's slice.
    pub index: usize,
    /// Bitrate in bits per second.
    pub bitrate: u32,
    /// Optional video width.
    pub width: Option<u32>,
    /// Optional video height.
    pub height: Option<u32>,
}

/// Selects the highest-quality variant that fits within `bandwidth_bps` (with a 15% safety
/// headroom) and `selection` caps, falling back to the lowest-bitrate variant if none fit.
pub fn select_abr_variant(
    variants: &[AbrVariant],
    bandwidth_bps: u32,
    selection: &TrackSelectionParameters,
) -> Option<usize> {
    if variants.is_empty() {
        return None;
    }
    let budget = (u64::from(bandwidth_bps) * 85) / 100;
    let eligible = variants.iter().filter(|v| {
        let fmt = TrackFormat {
            width: v.width,
            height: v.height,
            bitrate: Some(v.bitrate),
            ..TrackFormat::default()
        };
        selection.allows_video_format(&fmt)
    });
    let within_budget = eligible
        .clone()
        .filter(|v| u64::from(v.bitrate) <= budget)
        .max_by_key(|v| (v.bitrate, v.height.unwrap_or(0), v.width.unwrap_or(0)));
    if let Some(best) = within_budget {
        return Some(best.index);
    }
    eligible
        .min_by_key(|v| v.bitrate)
        .or_else(|| variants.iter().min_by_key(|v| v.bitrate))
        .map(|v| v.index)
}

fn parse_hls_rendition(base_url: &str, attrs: &str) -> Option<HlsRendition> {
    let kind = match find_attr(attrs, "TYPE")? {
        "AUDIO" => HlsRenditionType::Audio,
        "SUBTITLES" => HlsRenditionType::Subtitles,
        "VIDEO" => HlsRenditionType::Video,
        _ => return None,
    };
    Some(HlsRendition {
        kind,
        group_id: find_attr(attrs, "GROUP-ID")?.to_owned(),
        name: find_attr(attrs, "NAME").unwrap_or("").to_owned(),
        language: find_attr(attrs, "LANGUAGE").map(str::to_owned),
        uri: find_attr(attrs, "URI").map(|u| resolve_url(base_url, u)),
        is_default: find_attr(attrs, "DEFAULT") == Some("YES"),
    })
}

fn parse_hls_variant(base_url: &str, attrs: &str, uri: &str) -> Option<HlsVariant> {
    let bandwidth: u32 = find_attr(attrs, "BANDWIDTH")?.parse().ok()?;
    let average_bandwidth = find_attr(attrs, "AVERAGE-BANDWIDTH").and_then(|v| v.parse().ok());
    let resolution = find_attr(attrs, "RESOLUTION").and_then(|res| {
        let (w, h) = res.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    let frame_rate = find_attr(attrs, "FRAME-RATE").and_then(|v| v.parse().ok());
    Some(HlsVariant {
        uri: resolve_url(base_url, uri),
        bandwidth,
        average_bandwidth,
        resolution,
        codecs: find_attr(attrs, "CODECS").map(str::to_owned),
        frame_rate,
        audio_group: find_attr(attrs, "AUDIO").map(str::to_owned),
        subtitles_group: find_attr(attrs, "SUBTITLES").map(str::to_owned),
    })
}

fn find_attr<'a>(mut rest: &'a str, target_key: &str) -> Option<&'a str> {
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ']);
        let (key, after_eq) = rest.split_once('=')?;
        let (val, next) = if let Some(quoted) = after_eq.strip_prefix('"') {
            quoted.split_once('"').unwrap_or((quoted, ""))
        } else {
            after_eq
                .split_once(',')
                .map_or_else(|| (after_eq.trim(), ""), |(v, r)| (v.trim(), r))
        };
        if key.trim() == target_key {
            return Some(val);
        }
        rest = next;
    }
    None
}

fn parse_byte_range(val: &str, default_offset: u64) -> Option<(u64, u64)> {
    match val.split_once('@') {
        Some((len, off)) => Some((len.parse().ok()?, off.parse().ok()?)),
        None => Some((val.parse().ok()?, default_offset)),
    }
}

fn parse_dash_adaptation_set(
    period_base: &str,
    set: roxmltree::Node<'_, '_>,
) -> Option<DashAdaptationSet> {
    let set_base = resolve_base_url(set, period_base);
    let set_mime = set.attribute("mimeType");
    let set_codecs = set.attribute("codecs");
    let set_template = set
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "SegmentTemplate");

    let mut representations = Vec::new();
    for rep in set
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "Representation")
    {
        representations.push(parse_dash_representation(
            &set_base,
            rep,
            set_mime,
            set_codecs,
            set_template,
        ));
    }
    if representations.is_empty() {
        return None;
    }
    let track_type = infer_dash_track_type(
        set.attribute("contentType"),
        set_mime.or_else(|| representations[0].mime_type.as_deref()),
    );
    Some(DashAdaptationSet {
        track_type,
        language: set.attribute("lang").map(str::to_owned),
        representations,
    })
}

fn parse_dash_representation(
    set_base: &str,
    rep: roxmltree::Node<'_, '_>,
    set_mime: Option<&str>,
    set_codecs: Option<&str>,
    set_template: Option<roxmltree::Node<'_, '_>>,
) -> DashRepresentation {
    let id = rep.attribute("id").unwrap_or("").to_owned();
    let rep_base = resolve_base_url(rep, set_base);
    let template = rep
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "SegmentTemplate")
        .or(set_template);
    let (init_uri, segments) = match template {
        Some(tmpl) => expand_segment_template(&rep_base, &id, tmpl),
        None => parse_segment_list(&rep_base, rep),
    };
    DashRepresentation {
        id,
        bandwidth: rep
            .attribute("bandwidth")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        width: rep.attribute("width").and_then(|v| v.parse().ok()),
        height: rep.attribute("height").and_then(|v| v.parse().ok()),
        frame_rate: rep.attribute("frameRate").and_then(parse_frame_rate),
        codecs: rep.attribute("codecs").or(set_codecs).map(str::to_owned),
        mime_type: rep.attribute("mimeType").or(set_mime).map(str::to_owned),
        init_uri,
        segments,
    }
}

fn expand_segment_template(
    base_url: &str,
    rep_id: &str,
    tmpl: roxmltree::Node<'_, '_>,
) -> (Option<String>, Vec<DashSegment>) {
    let timescale: u64 = tmpl
        .attribute("timescale")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let start_number: u64 = tmpl
        .attribute("startNumber")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let init_uri = tmpl
        .attribute("initialization")
        .map(|pattern| resolve_url(base_url, &pattern.replace("$RepresentationID$", rep_id)));
    let Some(media_pattern) = tmpl.attribute("media") else {
        return (init_uri, Vec::new());
    };
    let mut segments = Vec::new();
    let Some(timeline) = tmpl
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "SegmentTimeline")
    else {
        return (init_uri, segments);
    };
    let mut current_time: u64 = 0;
    let mut number = start_number;
    for s in timeline
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "S")
    {
        if let Some(t) = s.attribute("t").and_then(|v| v.parse::<u64>().ok()) {
            current_time = t;
        }
        let Some(d) = s.attribute("d").and_then(|v| v.parse::<u64>().ok()) else {
            continue;
        };
        let repeat: u64 = s
            .attribute("r")
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|&r| r >= 0)
            .map_or(0, i64::cast_unsigned);
        let count = repeat.saturating_add(1).min(4096);
        let duration = Duration::from_secs_f64(d as f64 / timescale as f64);
        for _ in 0..count {
            let expanded = media_pattern
                .replace("$RepresentationID$", rep_id)
                .replace("$Number$", &number.to_string())
                .replace("$Time$", &current_time.to_string());
            segments.push(DashSegment {
                uri: resolve_url(base_url, &expanded),
                duration,
            });
            current_time = current_time.saturating_add(d);
            number = number.saturating_add(1);
        }
    }
    (init_uri, segments)
}

fn parse_segment_list(
    base_url: &str,
    rep: roxmltree::Node<'_, '_>,
) -> (Option<String>, Vec<DashSegment>) {
    let Some(list) = rep
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "SegmentList")
    else {
        return (None, Vec::new());
    };
    let timescale: u64 = list
        .attribute("timescale")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let seg_dur = list
        .attribute("duration")
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(Duration::ZERO, |d| {
            Duration::from_secs_f64(d as f64 / timescale as f64)
        });
    let init_uri = list
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "Initialization")
        .and_then(|n| n.attribute("sourceURL"))
        .map(|u| resolve_url(base_url, u));
    let segments = list
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "SegmentURL")
        .filter_map(|n| n.attribute("media"))
        .map(|u| DashSegment {
            uri: resolve_url(base_url, u),
            duration: seg_dur,
        })
        .collect();
    (init_uri, segments)
}

fn infer_dash_track_type(content_type: Option<&str>, mime_type: Option<&str>) -> TrackType {
    let hint = content_type.or(mime_type).unwrap_or("");
    if hint.starts_with("audio") {
        TrackType::Audio
    } else if hint.starts_with("text") || hint.contains("vtt") || hint.contains("ttml") {
        TrackType::Text
    } else {
        TrackType::Video
    }
}

fn parse_frame_rate(raw: &str) -> Option<f32> {
    if let Some((num, den)) = raw.split_once('/') {
        let n: f32 = num.parse().ok()?;
        let d: f32 = den.parse().ok()?;
        (d > 0.0 && n.is_finite() && d.is_finite()).then_some(n / d)
    } else {
        raw.parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0)
    }
}

fn parse_iso8601_duration(raw: &str) -> Option<Duration> {
    let rest = raw.strip_prefix("PT")?;
    let mut total = 0.0_f64;
    let mut buf = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
        } else {
            let val: f64 = buf.parse().ok()?;
            buf.clear();
            match ch {
                'H' => total += val * 3600.0,
                'M' => total += val * 60.0,
                'S' => total += val,
                _ => return None,
            }
        }
    }
    (total.is_finite() && total >= 0.0).then(|| Duration::from_secs_f64(total))
}

fn resolve_base_url(node: roxmltree::Node<'_, '_>, parent_base: &str) -> String {
    child_text(node, "BaseURL").map_or_else(
        || parent_base.to_owned(),
        |base| resolve_url(parent_base, base),
    )
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == tag)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn resolve_url(base_url: &str, relative: &str) -> String {
    if relative.contains("://") {
        return relative.to_owned();
    }
    url::Url::parse(base_url)
        .and_then(|base| base.join(relative))
        .map_or_else(|_| relative.to_owned(), String::from)
}
