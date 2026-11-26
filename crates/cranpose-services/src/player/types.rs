//! Media3-aligned player state, commands, events, error, and buffer control types.

use std::time::Duration;

use super::{CueGroup, Tracks};

/// Unique identifier for one [`Player`](super::Player) instance within the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerId(pub(super) u64);

/// Playback lifecycle state, matching `androidx.media3.common.Player` states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlayerState {
    /// Initial state, or after `stop()` / `release()` / fatal error.
    #[default]
    Idle,
    /// Opening a source or waiting for enough buffered media to begin or resume playback.
    Buffering,
    /// Prepared with buffered media and able to render from the current position immediately.
    Ready,
    /// Reached the end of the active playlist.
    Ended,
}

/// Repeat mode for playlist playback, matching `androidx.media3.common.Player.RepeatMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RepeatMode {
    /// Play the playlist once and stop at the end.
    #[default]
    Off,
    /// Repeat the current media item indefinitely.
    One,
    /// Repeat the entire playlist indefinitely.
    All,
}

impl RepeatMode {
    /// Advances to the next repeat mode in standard cycle order (`Off -> One -> All -> Off`).
    pub const fn next(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }
}

/// Playback speed and pitch multipliers, matching `androidx.media3.common.PlaybackParameters`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackParameters {
    /// Speed multiplier (`1.0` = normal speed, strictly positive).
    pub speed: f32,
    /// Audio pitch multiplier (`1.0` = unmodified pitch, strictly positive).
    pub pitch: f32,
}

impl PlaybackParameters {
    /// Normal 1.0x speed and unmodified pitch.
    pub const DEFAULT: Self = Self {
        speed: 1.0,
        pitch: 1.0,
    };

    /// Creates validated playback parameters, clamping non-finite or non-positive values into
    /// `[0.1, 16.0]`.
    pub fn new(speed: f32, pitch: f32) -> Self {
        Self {
            speed: clamp_rate(speed),
            pitch: clamp_rate(pitch),
        }
    }

    /// Creates parameters with `speed` and unmodified `1.0` pitch.
    pub fn with_speed(speed: f32) -> Self {
        Self::new(speed, 1.0)
    }
}

impl Default for PlaybackParameters {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn clamp_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(0.1, 16.0)
    } else {
        1.0
    }
}

/// Video display dimensions and orientation, matching `androidx.media3.common.VideoSize`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VideoSize {
    /// Width of the video frame in pixels.
    pub width: u32,
    /// Height of the video frame in pixels.
    pub height: u32,
    /// Clockwise rotation in degrees (`0`, `90`, `180`, `270`) not yet applied to pixel buffers.
    pub unapplied_rotation_degrees: u32,
    /// Ratio of pixel width to pixel height (`1.0` for square pixels).
    pub pixel_width_height_ratio: f32,
}

impl VideoSize {
    /// Unknown or absent video size.
    pub const ZERO: Self = Self {
        width: 0,
        height: 0,
        unapplied_rotation_degrees: 0,
        pixel_width_height_ratio: 1.0,
    };

    /// Creates a square-pixel, upright video size.
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            unapplied_rotation_degrees: 0,
            pixel_width_height_ratio: 1.0,
        }
    }

    /// Display aspect ratio (`width / height`), accounting for pixel aspect ratio and rotation,
    /// or `None` when either dimension is zero.
    pub fn display_aspect_ratio(&self) -> Option<f32> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let par =
            if self.pixel_width_height_ratio.is_finite() && self.pixel_width_height_ratio > 0.0 {
                self.pixel_width_height_ratio
            } else {
                1.0
            };
        let (w, h) = if self.unapplied_rotation_degrees % 180 == 90 {
            (self.height as f32, self.width as f32 * par)
        } else {
            (self.width as f32 * par, self.height as f32)
        };
        Some(w / h)
    }
}

/// Commands a [`Player`](super::Player) may advertise as available, matching Media3
/// `Player.Command`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PlayerCommand {
    /// Play or pause playback.
    PlayPause = 0,
    /// Prepare the active media item.
    Prepare = 1,
    /// Stop playback.
    Stop = 2,
    /// Seek within the current media item.
    SeekInCurrentMediaItem = 3,
    /// Seek backward by the configured increment.
    SeekBack = 4,
    /// Seek forward by the configured increment.
    SeekForward = 5,
    /// Advance to the next media item in the playlist.
    SeekToNext = 6,
    /// Return to the previous media item or restart the current item.
    SeekToPrevious = 7,
    /// Adjust playback speed and pitch.
    SetSpeedAndPitch = 8,
    /// Toggle shuffle mode.
    SetShuffleMode = 9,
    /// Change repeat mode.
    SetRepeatMode = 10,
    /// Change volume or mute state.
    SetVolume = 11,
    /// Change active track selection parameters.
    SetTrackSelectionParameters = 12,
}

/// Bitset of [`PlayerCommand`]s currently available on a [`Player`](super::Player).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AvailableCommands(u32);

impl AvailableCommands {
    /// Empty command set.
    pub const EMPTY: Self = Self(0);

    /// Set containing every command.
    pub const ALL: Self = Self((1 << 13) - 1);

    /// Whether `command` is currently enabled.
    pub const fn contains(self, command: PlayerCommand) -> bool {
        (self.0 & (1 << (command as u8))) != 0
    }

    /// Returns a copy with `command` enabled.
    pub const fn with(self, command: PlayerCommand) -> Self {
        Self(self.0 | (1 << (command as u8)))
    }

    /// Returns a copy with `command` disabled.
    pub const fn without(self, command: PlayerCommand) -> Self {
        Self(self.0 & !(1 << (command as u8)))
    }
}

/// Why a player's playback position changed discontinuously.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PositionDiscontinuityReason {
    /// Automatic transition to the next item or loop iteration.
    AutoTransition,
    /// Explicit seek call from the application or user.
    Seek,
    /// Seek target was adjusted to a keyframe or seekable range.
    SeekAdjustment,
    /// Skipped to another playlist item.
    Skip,
    /// The active media item was removed from the playlist.
    Remove,
    /// Internal discontinuity inside a live or segmented stream.
    Internal,
}

/// How a player interacts with OS audio focus and media session controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AudioRole {
    /// Primary foreground playback: participates in audio focus, ducking, and OS media controls.
    #[default]
    Primary,
    /// Secondary or feed preview player: does not hijack OS lock-screen media controls.
    Secondary,
    /// Permanently muted ambient or preloading player: requests no audio focus.
    Muted,
}

/// Error reported by a [`Player`](super::Player) when opening, decoding, or rendering fails.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlayerError {
    /// The source URI, container, or stream could not be opened or parsed.
    #[error("media source error: {0}")]
    Source(String),
    /// A network request failed while loading a manifest or segment.
    #[error("network error: {0}")]
    Network(String),
    /// The video or audio decoder failed.
    #[error("decoder error: {0}")]
    Decoder(String),
    /// The platform audio output failed.
    #[error("audio output error: {0}")]
    AudioOutput(String),
    /// This platform or media format is not supported.
    #[error("unsupported media playback: {0}")]
    Unsupported(String),
    /// The player has already been released.
    #[error("player has been released")]
    Released,
}

/// Reactive events published by a [`Player`](super::Player) on state transitions.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// Playback lifecycle state changed (`Idle`, `Buffering`, `Ready`, `Ended`).
    PlaybackStateChanged(PlayerState),
    /// `play_when_ready` intent changed.
    PlayWhenReadyChanged(bool),
    /// Effective `is_playing` state changed (`Ready && play_when_ready && no error`).
    IsPlayingChanged(bool),
    /// Whether the player is actively loading source data changed.
    IsLoadingChanged(bool),
    /// Active playlist index changed.
    MediaItemTransition {
        /// New active index in the playlist.
        index: usize,
    },
    /// Video dimensions or orientation changed.
    VideoSizeChanged(VideoSize),
    /// The zero-copy GPU frame stream backing `PlayerSurface` was attached, replaced, or cleared.
    VideoFramesChanged,
    /// The first GPU video frame for the active item has been rendered.
    RenderedFirstFrame,
    /// Available or selected tracks changed.
    TracksChanged(Tracks),
    /// Active subtitle/caption cues changed.
    CuesChanged(CueGroup),
    /// Playback speed or pitch changed.
    PlaybackParametersChanged(PlaybackParameters),
    /// Repeat mode changed.
    RepeatModeChanged(RepeatMode),
    /// Shuffle mode changed.
    ShuffleModeChanged(bool),
    /// Volume changed.
    VolumeChanged(f32),
    /// Muted state changed.
    MutedChanged(bool),
    /// Available commands bitset changed.
    AvailableCommandsChanged(AvailableCommands),
    /// Playback position jumped discontinuously.
    PositionDiscontinuity {
        /// Why the position jumped.
        reason: PositionDiscontinuityReason,
        /// New position after the jump.
        position: Duration,
    },
    /// Playback failed with a fatal error.
    Error(PlayerError),
}

/// Buffering thresholds controlling when a player starts, pauses for rebuffering, and caps memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadControl {
    /// Minimum buffered media duration to maintain before resuming network reads.
    pub min_buffer: Duration,
    /// Maximum buffered media duration to keep ahead of the playhead.
    pub max_buffer: Duration,
    /// Minimum buffered duration required before initial playback begins.
    pub buffer_for_playback: Duration,
    /// Minimum buffered duration required to resume after a rebuffer stall.
    pub buffer_for_playback_after_rebuffer: Duration,
}

impl LoadControl {
    /// Low-latency startup profile tuned for short-form vertical video feeds.
    pub const FEED: Self = Self {
        min_buffer: Duration::from_secs(3),
        max_buffer: Duration::from_secs(12),
        buffer_for_playback: Duration::from_millis(500),
        buffer_for_playback_after_rebuffer: Duration::from_millis(1500),
    };

    /// Creates a validated `LoadControl`, clamping inconsistent durations to valid bounds.
    pub fn new(
        min_buffer: Duration,
        max_buffer: Duration,
        buffer_for_playback: Duration,
        buffer_for_playback_after_rebuffer: Duration,
    ) -> Self {
        let min_buffer = min_buffer
            .max(buffer_for_playback)
            .max(buffer_for_playback_after_rebuffer);
        let max_buffer = max_buffer.max(min_buffer);
        Self {
            min_buffer,
            max_buffer,
            buffer_for_playback,
            buffer_for_playback_after_rebuffer,
        }
    }
}

impl Default for LoadControl {
    fn default() -> Self {
        Self {
            min_buffer: Duration::from_secs(15),
            max_buffer: Duration::from_secs(50),
            buffer_for_playback: Duration::from_millis(2500),
            buffer_for_playback_after_rebuffer: Duration::from_secs(5),
        }
    }
}
