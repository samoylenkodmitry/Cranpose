// The times of a gesture's samples on a platform that gives its pointer
// moves no event time: each sample is timed by when it is handled.
//
// A frame that runs long holds its moves back and they are then handled one
// after another, microseconds apart, though the finger made them a display
// frame or so apart. Timed by their handling they read as one fast flick.
// So a sample handled right after another is put one sample gap after it:
// the gap the gesture's samples kept while they arrived in time.

use cranpose_foundation::velocity_tracker::ASSUME_STOPPED_MS;
use web_time::{Duration, Instant};

// Samples handled closer together than this were held back: no pointer
// reports this often, a 1,000 Hz mouse reports every millisecond.
const HELD_BACK: Duration = Duration::from_micros(250);
// The gap between samples before the gesture showed its own: a 120 Hz
// touch panel's.
const FIRST_GAP_MS: i64 = 8;
// The longest gap taken as the gesture's own: a longer one is a stall.
const LONGEST_OWN_GAP_MS: i64 = 20;

pub(super) struct ArrivalClock {
    last_handled: Option<Instant>,
    gap_ms: i64,
}

impl Default for ArrivalClock {
    fn default() -> Self {
        Self {
            last_handled: None,
            gap_ms: FIRST_GAP_MS,
        }
    }
}

impl ArrivalClock {
    // The time, in milliseconds since the gesture began at `start`, of a
    // sample handled at `now`, after the one at `last_ms`.
    pub(super) fn sample_ms(&mut self, now: Instant, start: Instant, last_ms: Option<i64>) -> i64 {
        let held_back = self
            .last_handled
            .replace(now)
            .is_some_and(|before| now.saturating_duration_since(before) < HELD_BACK);
        let elapsed = now.saturating_duration_since(start).as_millis();
        let elapsed_ms = i64::try_from(elapsed).unwrap_or(i64::MAX);
        let Some(last_ms) = last_ms else {
            return elapsed_ms;
        };
        let gap_ms = elapsed_ms - last_ms;
        // A sample put ahead of the clock by an earlier burst leaves the
        // next one no gap of its own either.
        if held_back || gap_ms <= 0 {
            return last_ms + self.gap_ms;
        }
        if gap_ms <= LONGEST_OWN_GAP_MS {
            self.gap_ms = gap_ms;
        }
        last_ms + gap_ms.min(ASSUME_STOPPED_MS)
    }
}
