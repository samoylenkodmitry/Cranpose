//! Master presentation clock keeping audio and video synchronized within 40 ms.

use std::time::Duration;

use parking_lot::Mutex;
use web_time::Instant;

/// Maximum lip-sync drift tolerance (40 ms) before a late video frame is dropped.
pub const LIP_SYNC_TOLERANCE: Duration = Duration::from_millis(40);

/// Scheduling action for a decoded video frame compared against the active [`MasterClock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTiming {
    /// Display the frame now (on time or within acceptable sync tolerance).
    PresentNow,
    /// Frame is early; wait the given wall-clock duration before displaying it.
    Wait(Duration),
    /// Frame is later than [`LIP_SYNC_TOLERANCE`]; drop it and advance to the next frame.
    DropLate(Duration),
}

struct ClockState {
    anchor_media: Duration,
    anchor_wall: Instant,
    speed: f32,
    running: bool,
}

/// Shared per-player master clock driven by the audio output stream (or wall time for video-only
/// items).
///
/// Audio playback updates the anchor via [`sync_audio`](Self::sync_audio) as hardware output
/// buffers drain; video presentation reads [`position`](Self::position) and
/// [`frame_action`](Self::frame_action) to stay locked within [`LIP_SYNC_TOLERANCE`].
pub struct MasterClock {
    state: Mutex<ClockState>,
}

impl MasterClock {
    /// Creates a paused clock positioned at zero at `1.0x` speed.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ClockState {
                anchor_media: Duration::ZERO,
                anchor_wall: Instant::now(),
                speed: 1.0,
                running: false,
            }),
        }
    }

    /// Current media presentation position.
    pub fn position(&self) -> Duration {
        let guard = self.state.lock();
        guard.position_at(Instant::now())
    }

    /// Starts or resumes clock progression from its current position.
    pub fn resume(&self) {
        let now = Instant::now();
        let mut guard = self.state.lock();
        if !guard.running {
            guard.anchor_wall = now;
            guard.running = true;
        }
    }

    /// Pauses clock progression, freezing [`position`](Self::position) at the current instant.
    pub fn pause(&self) {
        let now = Instant::now();
        let mut guard = self.state.lock();
        if guard.running {
            guard.anchor_media = guard.position_at(now);
            guard.anchor_wall = now;
            guard.running = false;
        }
    }

    /// Jumps the clock to `position` (typically on seek or media item transition).
    pub fn seek(&self, position: Duration) {
        let now = Instant::now();
        let mut guard = self.state.lock();
        guard.anchor_media = position;
        guard.anchor_wall = now;
    }

    /// Anchors the clock to the actual audio hardware presentation timestamp.
    pub fn sync_audio(&self, audio_pts: Duration) {
        let now = Instant::now();
        let mut guard = self.state.lock();
        guard.anchor_media = audio_pts;
        guard.anchor_wall = now;
    }

    /// Changes playback speed without jumping the current media position.
    pub fn set_rate(&self, speed: f32) {
        let clamped = if speed.is_finite() {
            speed.clamp(0.1, 16.0)
        } else {
            1.0
        };
        let now = Instant::now();
        let mut guard = self.state.lock();
        guard.anchor_media = guard.position_at(now);
        guard.anchor_wall = now;
        guard.speed = clamped;
    }

    /// Active playback speed multiplier.
    pub fn rate(&self) -> f32 {
        self.state.lock().speed
    }

    /// Whether the clock is currently advancing.
    pub fn is_running(&self) -> bool {
        self.state.lock().running
    }

    /// Decides whether a video frame at `frame_pts` should be presented immediately, delayed, or
    /// dropped relative to the current master clock.
    pub fn frame_action(&self, frame_pts: Duration) -> FrameTiming {
        let (now_pos, speed) = {
            let guard = self.state.lock();
            (guard.position_at(Instant::now()), guard.speed)
        };
        if frame_pts >= now_pos {
            let ahead_media = frame_pts.saturating_sub(now_pos);
            let ahead_wall = Duration::from_secs_f64(ahead_media.as_secs_f64() / f64::from(speed));
            // Within half a 120 Hz frame (~4 ms) present immediately rather than oversleeping.
            if ahead_wall <= Duration::from_millis(4) {
                FrameTiming::PresentNow
            } else {
                FrameTiming::Wait(ahead_wall)
            }
        } else {
            let behind = now_pos.saturating_sub(frame_pts);
            if behind > LIP_SYNC_TOLERANCE {
                FrameTiming::DropLate(behind)
            } else {
                FrameTiming::PresentNow
            }
        }
    }
}

impl Default for MasterClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockState {
    fn position_at(&self, now: Instant) -> Duration {
        if !self.running {
            return self.anchor_media;
        }
        let elapsed = now.saturating_duration_since(self.anchor_wall);
        let scaled = Duration::from_secs_f64(elapsed.as_secs_f64() * f64::from(self.speed));
        self.anchor_media.saturating_add(scaled)
    }
}
