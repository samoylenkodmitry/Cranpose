//! Media3-aligned track discovery and runtime track selection types.

/// Kind of elementary media track inside a stream or playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackType {
    /// Visual video stream.
    Video,
    /// Audio stream.
    Audio,
    /// Timed text, subtitle, or closed caption stream.
    Text,
}

/// Descriptive format metadata for a single track inside a [`TrackGroup`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackFormat {
    /// Stable track identifier within its container or manifest.
    pub id: String,
    /// Optional MIME type (for example `"video/avc"`, `"audio/mp4a-latm"`, `"text/vtt"`).
    pub mime_type: Option<String>,
    /// Optional RFC 6381 codec string (for example `"avc1.640028"`, `"mp4a.40.2"`).
    pub codecs: Option<String>,
    /// Video width in pixels, if known.
    pub width: Option<u32>,
    /// Video height in pixels, if known.
    pub height: Option<u32>,
    /// Video frame rate in frames per second, if known.
    pub frame_rate: Option<f32>,
    /// Average or peak bitrate in bits per second, if known.
    pub bitrate: Option<u32>,
    /// Audio channel count, if known.
    pub channel_count: Option<u16>,
    /// Audio sample rate in Hz, if known.
    pub sample_rate: Option<u32>,
    /// Optional BCP-47 language tag (for example `"en"`, `"ja"`).
    pub language: Option<String>,
    /// Optional human-readable label (for example `"English (CC)"`, `"1080p60"`).
    pub label: Option<String>,
}

/// One selectable variant or stream within a [`TrackGroup`].
#[derive(Clone, Debug, PartialEq)]
pub struct TrackEntry {
    /// Format metadata for this track.
    pub format: TrackFormat,
    /// Whether this device can decode this track.
    pub supported: bool,
    /// Whether this track is currently selected for playback.
    pub selected: bool,
}

/// A group of mutually-related tracks (for example adaptive video renditions or one audio track).
#[derive(Clone, Debug, PartialEq)]
pub struct TrackGroup {
    /// Stable group identifier used by [`TrackSelectionOverride`].
    pub id: String,
    /// Kind of media carried by every track in this group.
    pub track_type: TrackType,
    /// Entries in this group.
    pub tracks: Vec<TrackEntry>,
    /// Whether adaptive switching between tracks in this group is supported.
    pub adaptive_supported: bool,
}

impl TrackGroup {
    /// Whether at least one track in this group is currently selected.
    pub fn is_selected(&self) -> bool {
        self.tracks.iter().any(|entry| entry.selected)
    }

    /// Returns the first selected track entry in this group, if any.
    pub fn selected_track(&self) -> Option<&TrackEntry> {
        self.tracks.iter().find(|entry| entry.selected)
    }
}

/// Snapshot of all available tracks and current selections for the active media item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    /// All discovered track groups.
    pub groups: Vec<TrackGroup>,
}

impl Tracks {
    /// Empty track set.
    pub const EMPTY: Self = Self { groups: Vec::new() };

    /// Whether no track groups are present.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Whether at least one group of `track_type` exists.
    pub fn contains_type(&self, track_type: TrackType) -> bool {
        self.groups
            .iter()
            .any(|group| group.track_type == track_type)
    }

    /// Whether at least one track of `track_type` is currently selected.
    pub fn is_type_selected(&self, track_type: TrackType) -> bool {
        self.groups
            .iter()
            .any(|group| group.track_type == track_type && group.is_selected())
    }

    /// Iterates over every group matching `track_type`.
    pub fn groups_of_type(&self, track_type: TrackType) -> impl Iterator<Item = &TrackGroup> {
        self.groups
            .iter()
            .filter(move |group| group.track_type == track_type)
    }
}

/// Explicit track override pinpointing specific track indices inside one [`TrackGroup`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSelectionOverride {
    /// Identifier of the target [`TrackGroup`].
    pub group_id: String,
    /// Indices within [`TrackGroup::tracks`] to force-select (empty disables the group).
    pub track_indices: Vec<usize>,
}

/// Constraints and preferences guiding automatic and manual track selection, matching Media3
/// `TrackSelectionParameters`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackSelectionParameters {
    /// Upper bound on selected video width in pixels.
    pub max_video_width: Option<u32>,
    /// Upper bound on selected video height in pixels.
    pub max_video_height: Option<u32>,
    /// Upper bound on selected video bitrate in bits per second.
    pub max_video_bitrate: Option<u32>,
    /// Preferred audio BCP-47 languages in priority order.
    pub preferred_audio_languages: Vec<String>,
    /// Preferred subtitle/caption BCP-47 languages in priority order.
    pub preferred_text_languages: Vec<String>,
    /// Track types explicitly disabled by the user (for example turning captions off).
    pub disabled_track_types: Vec<TrackType>,
    /// Explicit per-group track selection overrides.
    pub overrides: Vec<TrackSelectionOverride>,
}

impl TrackSelectionParameters {
    /// Whether `track_type` is disabled.
    pub fn is_type_disabled(&self, track_type: TrackType) -> bool {
        self.disabled_track_types.contains(&track_type)
    }

    /// Returns a copy with `max_video_width` and `max_video_height` constrained.
    pub fn with_max_video_size(mut self, width: u32, height: u32) -> Self {
        self.max_video_width = Some(width);
        self.max_video_height = Some(height);
        self
    }

    /// Returns a copy with `max_video_bitrate` constrained.
    pub fn with_max_video_bitrate(mut self, bitrate: u32) -> Self {
        self.max_video_bitrate = Some(bitrate);
        self
    }

    /// Returns a copy preferring `language` for audio tracks.
    pub fn with_preferred_audio_language(mut self, language: impl Into<String>) -> Self {
        self.preferred_audio_languages = vec![language.into()];
        self
    }

    /// Returns a copy preferring `language` for text/subtitle tracks.
    pub fn with_preferred_text_language(mut self, language: impl Into<String>) -> Self {
        self.preferred_text_languages = vec![language.into()];
        self
    }

    /// Returns a copy with `track_type` disabled.
    pub fn with_disabled_type(mut self, track_type: TrackType) -> Self {
        if !self.disabled_track_types.contains(&track_type) {
            self.disabled_track_types.push(track_type);
        }
        self
    }

    /// Returns a copy with `track_type` enabled (removed from `disabled_track_types`).
    pub fn with_enabled_type(mut self, track_type: TrackType) -> Self {
        self.disabled_track_types.retain(|t| *t != track_type);
        self
    }

    /// Returns a copy with an explicit [`TrackSelectionOverride`] set (replacing any previous
    /// override for the same `group_id`).
    pub fn with_override(mut self, selection: TrackSelectionOverride) -> Self {
        self.overrides.retain(|o| o.group_id != selection.group_id);
        self.overrides.push(selection);
        self
    }

    /// Checks whether a video format satisfies the configured width, height, and bitrate caps.
    pub fn allows_video_format(&self, format: &TrackFormat) -> bool {
        if self.is_type_disabled(TrackType::Video) {
            return false;
        }
        let width_ok = match (self.max_video_width, format.width) {
            (Some(limit), Some(w)) => w <= limit,
            _ => true,
        };
        let height_ok = match (self.max_video_height, format.height) {
            (Some(limit), Some(h)) => h <= limit,
            _ => true,
        };
        let bitrate_ok = match (self.max_video_bitrate, format.bitrate) {
            (Some(limit), Some(b)) => b <= limit,
            _ => true,
        };
        width_ok && height_ok && bitrate_ok
    }
}
