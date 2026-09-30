pub(crate) const WINDOW: i64 = 90;

/// Frames shown after the lead changes that started before it did, left out
/// of the next window.
pub(crate) const SETTLE: u32 = 6;

/// How much sooner, on average, a trial's frames must reach the screen for
/// its lead to be kept: less is noise.
const MARGIN_NS: i64 = 1_000_000;

/// The leads a frame may start at, in tenths of the refresh period. A short
/// frame on the Pixel 9 Pro needs 0.3; on the Mate 20 X a ticker frame
/// needs 0.3 too, while grid frames, whose GPU work runs 6 ms past their
/// queueing, need 0.5. Longer leads left feed missing vsyncs.
const LEADS_TENTHS: [i64; 3] = [0, 3, 5];

/// How long the kept lead runs before another is tried, and the longest
/// that grows to while trials keep failing.
const FIRST_HOLD_NS: i64 = 2_000_000_000;
const LONGEST_HOLD_NS: i64 = 32_000_000_000;

/// Pins the lead, in tenths of a period, instead of learning it: for
/// measuring what each lead does to a workload. Android sets it from
/// `debug.cranpose.frame_lead`.
const PINNED_LEAD_ENV: &str = "CRANPOSE_FRAME_LEAD_TENTHS";

pub(crate) struct FrameLead {
    /// The lead [`PINNED_LEAD_ENV`] pins, in tenths of a period.
    pinned_tenths: Option<i64>,
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
        Self::pinned_at(
            std::env::var(PINNED_LEAD_ENV)
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok()),
        )
    }
}

impl FrameLead {
    /// A lead that learns, or stays at `tenths` of a period when that is a
    /// lead a frame can take (0 to 9).
    pub(crate) fn pinned_at(tenths: Option<i64>) -> Self {
        Self {
            pinned_tenths: tenths.filter(|tenths| (0..10).contains(tenths)),
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

    /// How long before its slot's vsync a frame starts, for a display
    /// refreshing every `vsync_period_ns`.
    pub(crate) fn lead_ns(&self, vsync_period_ns: i64) -> i64 {
        let tenths = self
            .pinned_tenths
            .unwrap_or_else(|| LEADS_TENTHS[self.trial.unwrap_or(self.kept)]);
        vsync_period_ns * tenths / 10
    }

    pub(crate) fn record(&mut self, present_return_to_display_ns: i64, now_ns: i64) {
        if self.pinned_tenths.is_some() {
            return;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return;
        }
        self.sum_ns = self.sum_ns.saturating_add(present_return_to_display_ns);
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

    /// Goes back to starting frames on their slot and forgets the window in
    /// progress and any trial. A lead the frames cannot keep up with shows as
    /// missed vsyncs, and missed vsyncs are what make the pacer rise a level.
    pub(crate) fn fall_back(&mut self) {
        self.kept = 0;
        self.hold_ns = FIRST_HOLD_NS;
        self.next_trial_ns = None;
        self.reset();
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
