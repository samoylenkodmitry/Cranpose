use std::time::Duration;

use web_time::Instant;

use crate::FrameUpdateResult;

/// How long input, moving content or a continuous redraw keeps the fast rate.
const BOOST_HOLD_OFF: Duration = Duration::from_secs(3);

/// Android's `ViewRootImpl` counts a window as redrawn continuously while its
/// last two frame intervals add up to less than this.
const CONTINUOUS_REDRAW_SPAN: Duration = Duration::from_millis(100);

/// When [`FrameRatePreference::Auto`](crate::FrameRatePreference::Auto) asks
/// for the display's fastest rate, following how Android's views vote a
/// frame-rate category: input, content that moves, and a large view redrawn
/// continuously all vote high. A Cranpose window is one screen-sized view, as
/// a Compose window is, so continuous redraws boost it and intermittent ones,
/// such as a blinking cursor, do not. A boost holds for a few seconds, so the
/// display does not flap between rates.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameRateBoost {
    boosted_at: Option<Instant>,
    redraws: [Option<Instant>; 2],
}

impl FrameRateBoost {
    /// Records input that arrived at `now`.
    pub fn note_input(&mut self, now: Instant) {
        self.boosted_at = Some(now);
    }

    /// Records the frame an update produced at `now`.
    pub fn note_frame(&mut self, result: FrameUpdateResult, now: Instant) {
        if result.content_moved {
            self.boosted_at = Some(now);
        }
        if result.content_redrawn {
            let [older, newer] = self.redraws;
            if older.is_some_and(|at| now.saturating_duration_since(at) < CONTINUOUS_REDRAW_SPAN) {
                self.boosted_at = Some(now);
            }
            self.redraws = [newer, Some(now)];
        }
    }

    /// Whether the fast rate still holds at `now`.
    pub fn boosted(&self, now: Instant) -> bool {
        self.boosted_at
            .is_some_and(|at| now.saturating_duration_since(at) < BOOST_HOLD_OFF)
    }
}

#[cfg(test)]
#[path = "tests/frame_rate_boost_tests.rs"]
mod tests;
