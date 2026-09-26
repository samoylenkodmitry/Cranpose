//! How far ahead of its vsync slot a paced frame starts.
//!
//! The compositor shows a frame at the earliest refresh only when the frame
//! is ready by that refresh's deadline, its GPU work included; on a Pixel 9
//! Pro at 120 Hz the deadline falls about 8.3 ms after the vsync callback a
//! frame starts on, and a frame ready a little later waits a whole refresh
//! more. Starting a share of a period early catches the earlier refresh when
//! the frame is short enough. A longer frame only starts early and then
//! waits as long, packed closer to the frame before it, and how early is
//! enough depends on how long the frame and its GPU work take, so the lead
//! is measured rather than assumed: now and then a window of frames runs at
//! one of the other leads, and the lead whose frames reached the screen
//! sooner after being queued is kept.

/// Shown frames whose queue-to-screen times are averaged per window.
const WINDOW: i64 = 90;

/// Frames shown after the lead changes that started before it did, left out
/// of the next window.
const SETTLE: u32 = 6;

/// How much sooner, on average, a trial's frames must reach the screen for
/// its lead to be kept: less is noise.
const MARGIN_NS: i64 = 1_000_000;

/// The leads a frame may start at, in tenths of the refresh period. A short
/// frame on the Pixel 9 Pro needs 0.3; on the Mate 20 X a ticker frame
/// needs 0.3 too, while grid and feed frames, whose GPU work runs 6 to 9 ms
/// past their queueing, gain nothing from it.
const LEADS_TENTHS: [i64; 4] = [0, 3, 5, 7];

/// How long the kept lead runs before another is tried, and the longest
/// that grows to while trials keep failing.
const FIRST_HOLD_NS: i64 = 2_000_000_000;
const LONGEST_HOLD_NS: i64 = 32_000_000_000;

pub(crate) struct FrameLead {
    /// The lead kept, as an index into [`LEADS_TENTHS`].
    kept: usize,
    /// The lead on trial, if a trial runs.
    trial: Option<usize>,
    /// The lead tried last, so trials take the other leads in turn.
    last_tried: usize,
    settle: u32,
    sum_ns: i64,
    count: i64,
    baseline_ns: Option<i64>,
    next_trial_ns: Option<i64>,
    hold_ns: i64,
}

impl Default for FrameLead {
    fn default() -> Self {
        Self {
            kept: 0,
            trial: None,
            last_tried: 0,
            settle: 0,
            sum_ns: 0,
            count: 0,
            baseline_ns: None,
            next_trial_ns: None,
            hold_ns: FIRST_HOLD_NS,
        }
    }
}

impl FrameLead {
    /// How long before its slot's vsync a frame starts, for a display
    /// refreshing every `vsync_period_ns`.
    pub(crate) fn lead_ns(&self, vsync_period_ns: i64) -> i64 {
        vsync_period_ns * LEADS_TENTHS[self.trial.unwrap_or(self.kept)] / 10
    }

    /// Records how long a frame took from being queued to being shown, at
    /// `now_ns`: closes a window every [`WINDOW`] frames, starts a trial of
    /// another lead once the kept one has held long enough, and keeps the
    /// trial's lead when it brought frames to the screen sooner.
    pub(crate) fn record(&mut self, latency_ns: i64, now_ns: i64) {
        if self.settle > 0 {
            self.settle -= 1;
            return;
        }
        self.sum_ns = self.sum_ns.saturating_add(latency_ns);
        self.count += 1;
        if self.count < WINDOW {
            return;
        }
        let mean_ns = self.sum_ns / WINDOW;
        self.sum_ns = 0;
        self.count = 0;
        if let Some(trial) = self.trial.take() {
            self.end_trial(trial, mean_ns, now_ns);
            return;
        }
        self.baseline_ns = Some(mean_ns);
        if self.next_trial_ns.is_none_or(|at| now_ns >= at) {
            self.trial = Some(self.next_lead_to_try());
            self.settle = SETTLE;
        }
    }

    /// Keeps `trial`'s lead when its window's mean beat the kept lead's by
    /// the margin. Every failed trial doubles the hold before the next: a
    /// trial of a worse lead costs its window's frames, and a scene that has
    /// settled on its lead should not keep paying for trials.
    fn end_trial(&mut self, trial: usize, mean_ns: i64, now_ns: i64) {
        if self
            .baseline_ns
            .is_some_and(|baseline| mean_ns + MARGIN_NS < baseline)
        {
            self.kept = trial;
            self.baseline_ns = Some(mean_ns);
            self.hold_ns = FIRST_HOLD_NS;
        } else {
            self.settle = SETTLE;
            self.hold_ns = (self.hold_ns * 2).min(LONGEST_HOLD_NS);
        }
        self.next_trial_ns = Some(now_ns + self.hold_ns);
    }

    /// The next lead other than the kept one, in turn after the last tried.
    fn next_lead_to_try(&mut self) -> usize {
        let mut next = (self.last_tried + 1) % LEADS_TENTHS.len();
        if next == self.kept {
            next = (next + 1) % LEADS_TENTHS.len();
        }
        self.last_tried = next;
        next
    }

    /// Forgets the window in progress and any trial, keeping the lead
    /// learned so far: latencies from before a change of pacing level say
    /// nothing about frames after it.
    pub(crate) fn reset(&mut self) {
        self.trial = None;
        self.settle = SETTLE;
        self.sum_ns = 0;
        self.count = 0;
        self.baseline_ns = None;
    }
}

#[cfg(test)]
#[path = "tests/frame_lead_tests.rs"]
mod tests;
