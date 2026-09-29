//! How long an unpaced loop holds a due frame back.
//!
//! A loop runs unpaced when it cannot keep up with the display. When what
//! holds it back is the GPU, the present thread spends each frame blocked
//! in the driver while the frame it takes next waits for it, already
//! built: every frame is built well before the GPU can start it, and
//! reaches the screen that much later. Holding each frame back, after the
//! renderer hands a frame back, by the time frames would wait anyway builds
//! them that much later at the same rate.
//!
//! The hold is measured rather than assumed. Each frame reports how long it
//! waited from its hand-off to the present thread until its image was
//! acquired; every window the hold moves toward leaving the shortest waits
//! at a small target, so the renderer never waits on the loop for long. It
//! grows by half the surplus and shrinks by the whole shortfall: a hold too
//! long costs frames, one too short only latency.

/// Frames whose waits are compared before the hold moves.
pub(crate) const WINDOW: usize = 30;

/// How long the shortest waits of a window are kept: enough that a frame
/// that takes a little longer to build still arrives before the renderer
/// is free.
pub(crate) const TARGET_WAIT_NS: i64 = 2_000_000;

#[derive(Default)]
pub(crate) struct UnpacedHold {
    hold_ns: i64,
    waits: Vec<i64>,
}

impl UnpacedHold {
    /// How long a due frame waits after the renderer hands a frame back.
    pub(crate) fn hold_ns(&self) -> i64 {
        self.hold_ns
    }

    /// Records how long a frame waited from its hand-off until its image
    /// was acquired, on a display refreshing every `vsync_period_ns`. Every
    /// [`WINDOW`] frames the hold moves by how far the window's
    /// tenth-shortest wait is from [`TARGET_WAIT_NS`], and stays within a
    /// refresh period.
    pub(crate) fn record_wait(&mut self, wait_ns: i64, vsync_period_ns: i64) {
        self.waits.push(wait_ns.max(0));
        if self.waits.len() < WINDOW {
            return;
        }
        self.waits.sort_unstable();
        let shortest = self.waits[WINDOW / 10];
        self.waits.clear();
        let surplus = shortest - TARGET_WAIT_NS;
        let step = if surplus > 0 { surplus / 2 } else { surplus };
        self.hold_ns = (self.hold_ns + step).clamp(0, vsync_period_ns.max(0));
    }

    /// Stops holding frames and forgets the window in progress.
    pub(crate) fn reset(&mut self) {
        self.hold_ns = 0;
        self.waits.clear();
    }
}

#[cfg(test)]
#[path = "tests/unpaced_hold_tests.rs"]
mod tests;
