//! Timed text cues and zero-dependency SRT and WebVTT subtitle parsers.

use std::time::Duration;

/// Horizontal alignment of a subtitle [`Cue`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CueAlignment {
    /// Aligned to the leading edge.
    Start,
    /// Centered horizontally (standard subtitle default).
    #[default]
    Center,
    /// Aligned to the trailing edge.
    End,
}

/// One timed subtitle or closed-caption cue.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    /// Plain display text with inline HTML/WebVTT markup tags stripped.
    pub text: String,
    /// Optional normalized vertical position in `[0.0, 1.0]` (`0.0` = top, `1.0` = bottom).
    pub line: Option<f32>,
    /// Optional normalized horizontal anchor in `[0.0, 1.0]`.
    pub position: Option<f32>,
    /// Optional normalized width fraction in `(0.0, 1.0]`.
    pub size: Option<f32>,
    /// Horizontal text alignment within the cue box.
    pub text_alignment: CueAlignment,
    /// Inclusive presentation start time.
    pub start_time: Duration,
    /// Exclusive presentation end time.
    pub end_time: Duration,
}

/// Set of cues active at a specific presentation timestamp, matching Media3 `CueGroup`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueGroup {
    /// Active cues to render simultaneously.
    pub cues: Vec<Cue>,
    /// Presentation timestamp for which these cues were computed.
    pub presentation_time: Duration,
}

impl CueGroup {
    /// Empty cue group at timestamp zero.
    pub const EMPTY: Self = Self {
        cues: Vec::new(),
        presentation_time: Duration::ZERO,
    };

    /// Whether no cues are active.
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }
}

/// Parsed subtitle timeline supporting O(log n) lookup of active cues at any playback position.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubtitleTrack {
    cues: Vec<Cue>,
    max_cue_duration: Duration,
}

impl SubtitleTrack {
    /// Builds a sorted [`SubtitleTrack`] from parsed cues.
    pub fn from_cues(mut cues: Vec<Cue>) -> Self {
        cues.retain(|cue| !cue.text.is_empty() && cue.end_time > cue.start_time);
        cues.sort_by_key(|cue| (cue.start_time, cue.end_time));
        let max_cue_duration = cues
            .iter()
            .map(|cue| cue.end_time.saturating_sub(cue.start_time))
            .max()
            .unwrap_or(Duration::ZERO);
        Self {
            cues,
            max_cue_duration,
        }
    }

    /// All parsed cues in ascending start-time order.
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// Whether the track contains no valid cues.
    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    /// Returns the [`CueGroup`] active at `position` (`start_time <= position < end_time`).
    pub fn cues_at(&self, position: Duration) -> CueGroup {
        if self.cues.is_empty() {
            return CueGroup {
                cues: Vec::new(),
                presentation_time: position,
            };
        }
        let upper = self.cues.partition_point(|cue| cue.start_time <= position);
        let earliest = position.saturating_sub(self.max_cue_duration);
        let lower = self.cues[..upper].partition_point(|cue| cue.start_time < earliest);
        let active = self.cues[lower..upper]
            .iter()
            .filter(|cue| cue.start_time <= position && position < cue.end_time)
            .cloned()
            .collect();
        CueGroup {
            cues: active,
            presentation_time: position,
        }
    }
}

/// Parses either WebVTT or SubRip (SRT) based on `hint` (MIME type, URI, or extension) or header.
pub fn parse_subtitles(hint: Option<&str>, text: &str) -> SubtitleTrack {
    let stripped = text.trim_start_matches('\u{feff}');
    let looks_vtt = stripped.trim_start().starts_with("WEBVTT")
        || hint.is_some_and(|h| {
            let lower = h.to_ascii_lowercase();
            lower.contains("vtt")
        });
    if looks_vtt {
        parse_webvtt(stripped)
    } else {
        parse_srt(stripped)
    }
}

/// Parses a SubRip (`.srt`) subtitle document into a [`SubtitleTrack`].
pub fn parse_srt(input: &str) -> SubtitleTrack {
    let text = input.trim_start_matches('\u{feff}');
    let mut cues = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let timing_line = if trimmed.contains("-->") {
            trimmed
        } else if lines.peek().is_some_and(|next| next.contains("-->")) {
            lines.next().unwrap_or("").trim()
        } else {
            continue;
        };
        let Some((start_time, end_time, _settings)) = parse_timing_line(timing_line) else {
            continue;
        };
        let payload = collect_cue_text(&mut lines);
        if !payload.is_empty() {
            cues.push(Cue {
                text: payload,
                line: None,
                position: None,
                size: None,
                text_alignment: CueAlignment::Center,
                start_time,
                end_time,
            });
        }
    }
    SubtitleTrack::from_cues(cues)
}

/// Parses a WebVTT (`.vtt`) subtitle document into a [`SubtitleTrack`].
pub fn parse_webvtt(input: &str) -> SubtitleTrack {
    let text = input.trim_start_matches('\u{feff}');
    let mut cues = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("WEBVTT") {
            continue;
        }
        if trimmed.starts_with("NOTE")
            || trimmed.starts_with("STYLE")
            || trimmed.starts_with("REGION")
        {
            skip_until_blank(&mut lines);
            continue;
        }
        let timing_line = if trimmed.contains("-->") {
            trimmed
        } else if lines.peek().is_some_and(|next| next.contains("-->")) {
            lines.next().unwrap_or("").trim()
        } else {
            continue;
        };
        let Some((start_time, end_time, settings)) = parse_timing_line(timing_line) else {
            skip_until_blank(&mut lines);
            continue;
        };
        let payload = collect_cue_text(&mut lines);
        if !payload.is_empty() {
            let (line_pos, position, size, text_alignment) = parse_vtt_settings(settings);
            cues.push(Cue {
                text: payload,
                line: line_pos,
                position,
                size,
                text_alignment,
                start_time,
                end_time,
            });
        }
    }
    SubtitleTrack::from_cues(cues)
}

fn skip_until_blank(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) {
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            break;
        }
    }
}

fn collect_cue_text(lines: &mut std::iter::Peekable<std::str::Lines<'_>>) -> String {
    let mut out = String::new();
    for line in lines.by_ref() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.trim().is_empty() {
            break;
        }
        let clean = strip_markup_tags(trimmed);
        if !clean.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&clean);
        }
    }
    out
}

fn parse_timing_line(line: &str) -> Option<(Duration, Duration, &str)> {
    let (lhs, rhs_all) = line.split_once("-->")?;
    let start = parse_timestamp(lhs.trim())?;
    let rhs_trimmed = rhs_all.trim_start();
    let (end_token, settings) = match rhs_trimmed.split_once(char::is_whitespace) {
        Some((end, rest)) => (end, rest.trim()),
        None => (rhs_trimmed, ""),
    };
    let end = parse_timestamp(end_token)?;
    Some((start, end, settings))
}

fn parse_timestamp(token: &str) -> Option<Duration> {
    let (hms, frac) = token
        .split_once('.')
        .or_else(|| token.split_once(','))
        .unwrap_or((token, "0"));
    let mut parts = hms.split(':');
    let first: u64 = parts.next()?.parse().ok()?;
    let second: u64 = parts.next()?.parse().ok()?;
    let (hours, minutes, seconds) = match parts.next() {
        Some(third) => (first, second, third.parse::<u64>().ok()?),
        None => (0, first, second),
    };
    if parts.next().is_some() {
        return None;
    }
    let millis = parse_millis(frac)?;
    let total_ms = hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(millis)?;
    Some(Duration::from_millis(total_ms))
}

fn parse_millis(frac: &str) -> Option<u64> {
    let digits: String = frac.chars().take(3).collect();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let raw: u64 = digits.parse().ok()?;
    Some(match digits.len() {
        1 => raw * 100,
        2 => raw * 10,
        _ => raw,
    })
}

fn parse_vtt_settings(settings: &str) -> (Option<f32>, Option<f32>, Option<f32>, CueAlignment) {
    let mut line = None;
    let mut position = None;
    let mut size = None;
    let mut alignment = CueAlignment::Center;
    for token in settings.split_whitespace() {
        let Some((key, val)) = token.split_once(':') else {
            continue;
        };
        match key {
            "line" => line = parse_percentage(val),
            "position" => position = parse_percentage(val),
            "size" => size = parse_percentage(val),
            "align" => {
                alignment = match val {
                    "start" | "left" => CueAlignment::Start,
                    "end" | "right" => CueAlignment::End,
                    _ => CueAlignment::Center,
                };
            }
            _ => {}
        }
    }
    (line, position, size, alignment)
}

fn parse_percentage(value: &str) -> Option<f32> {
    let primary = value.split(',').next()?;
    let pct = primary.strip_suffix('%')?;
    let parsed: f32 = pct.parse().ok()?;
    parsed.is_finite().then(|| (parsed / 100.0).clamp(0.0, 1.0))
}

fn strip_markup_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_basic_entities(&out)
}

fn decode_basic_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
}
