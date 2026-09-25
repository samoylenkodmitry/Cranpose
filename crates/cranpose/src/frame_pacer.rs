//! Paces a frame loop to the display.
//!
//! The display takes one image per vsync, so a loop that starts frames
//! faster than that only fills the swapchain queue, and every frame then
//! waits behind the queued ones to reach the screen. So at most one frame
//! starts per vsync slot, and a frame that ran long starts its successor as
//! soon as the next slot has opened, the way Android's own renderer paces.
//!
//! Pacing alone keeps whatever depth the queue already has: the loop adds a
//! frame per vsync as fast as the display takes one, and the frames started
//! before the display first reports are enough to fill it. The display says
//! how deep it is: when it shows a frame, how many later ones are queued
//! behind it. When recent frames all had more behind them than the queue is
//! meant to hold, the loop gives up one slot and the queue drains by a frame.
//! That costs one frame's content once, not frame rate: the queue still
//! holds a frame for the slot the skip leaves empty.
//!
//! How deep the queue is meant to be depends on the frames. A loop that
//! cannot keep up with the display never fills it, so pacing it gains
//! nothing and only takes away the overlap between frames; such a loop runs
//! unpaced, starting frames as soon as the renderer takes them. A loop that
//! can keep up does fill it, and the display reports three or more frames
//! behind the shown one. Then it is paced with two frames queued, and later
//! one. One gets frames to the screen soonest, but leaves nothing to show
//! when a frame runs late, which the display reports as a vsync no new frame
//! reached. Missed vsyncs move the loop up a level; after a while it tries
//! the level below again, gives the try up at its first missed vsync, and
//! waits four times as long before the next try when that happens soon.
//!
//! Until the display has reported a frame, and on a swapchain that never
//! does, frames start as soon as the renderer takes them.

/// Frames whose queue depths are compared before draining a paced queue.
const HISTORY: usize = 3;

/// Frames whose queue depths must all be over two before an unpaced loop
/// counts as filling the queue. A loop that only now and then gets ahead of
/// the display, as one slower than it does, is left alone.
const FULL_HISTORY: usize = 16;

/// Presents remembered while their display times are awaited; the display
/// reports a frame a few presents after it.
const PRESENT_LOG: usize = 16;

/// The shortest gap between two skips. The display reports a frame a few
/// vsyncs after it was shown, so a skip's effect shows only after that.
const SKIP_SPACING_NS: i64 = 500_000_000;

/// How early a wake may arrive and still start the slot it was for.
const SLOT_TOLERANCE_NS: i64 = 1_000_000;

/// Vsyncs after which what the display said about a frame no longer
/// describes the queue: the display reports a frame a vsync or two after
/// showing it, and a loop idle this long has let the queue drain.
const SHOWN_FRESH_VSYNCS: i64 = 6;

/// Shown frames over which missed vsyncs are counted.
const MISS_WINDOW: usize = 64;

/// How long the loop first stays a level up before trying the level below,
/// and the longest that grows to when the level below keeps failing soon.
const FIRST_HOLD_NS: i64 = 4_000_000_000;
const LONGEST_HOLD_NS: i64 = 64_000_000_000;

/// A stretch this long at a level counts as that level working, so the next
/// failure starts again from the first hold.
const STABLE_LEVEL_NS: i64 = 5_000_000_000;

/// Recent presents by id, to count how many followed a frame into the
/// queue before the display showed it.
#[derive(Default)]
pub(crate) struct PresentLog {
    presents: std::collections::VecDeque<(u32, i64)>,
}

impl PresentLog {
    /// Remembers that present `id` returned at `presented_ns`.
    pub(crate) fn record(&mut self, id: u32, presented_ns: i64) {
        if self.presents.len() == PRESENT_LOG {
            self.presents.pop_front();
        }
        self.presents.push_back((id, presented_ns));
    }

    /// How many later presents were queued when present `id` was shown at
    /// `shown_ns`, or `None` once `id` is forgotten. The display reports
    /// frames in order, so `id` and every present before it are dropped.
    pub(crate) fn queued_behind(&mut self, id: u32, shown_ns: i64) -> Option<u32> {
        let position = self
            .presents
            .iter()
            .position(|(present, _)| *present == id)?;
        let behind = self
            .presents
            .iter()
            .skip(position + 1)
            .filter(|(_, presented)| *presented < shown_ns)
            .count();
        self.presents.drain(..=position);
        u32::try_from(behind).ok()
    }
}

/// How far ahead of the display frames run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// One frame queued behind the shown one.
    Shallow,
    /// Two, so one late frame leaves another to show.
    Buffered,
    /// As many as the swapchain holds.
    Unpaced,
}

impl Level {
    /// Frames queued behind the shown one that this level aims for, or
    /// `None` for no aim at all.
    fn depth(self) -> Option<u32> {
        match self {
            Self::Shallow => Some(1),
            Self::Buffered => Some(2),
            Self::Unpaced => None,
        }
    }

    /// Missed vsyncs within the window that move the loop up from here: the
    /// first one while the loop is trying again a level that failed.
    fn misses_to_rise(self, retrying: bool) -> Option<usize> {
        let settled = match self {
            Self::Shallow => 2,
            Self::Buffered => 6,
            Self::Unpaced => return None,
        };
        Some(if retrying { 1 } else { settled })
    }

    /// Where this level's failures are remembered, if it can fail.
    fn failure_slot(self) -> Option<usize> {
        match self {
            Self::Shallow => Some(0),
            Self::Buffered => Some(1),
            Self::Unpaced => None,
        }
    }

    fn above(self) -> Self {
        match self {
            Self::Shallow => Self::Buffered,
            Self::Buffered | Self::Unpaced => Self::Unpaced,
        }
    }

    fn below(self) -> Self {
        match self {
            Self::Unpaced => Self::Buffered,
            Self::Buffered | Self::Shallow => Self::Shallow,
        }
    }
}

/// The level frames run at, since when, and until when before the level
/// below is tried again.
#[derive(Clone, Copy)]
struct Stage {
    level: Level,
    since_ns: i64,
    until_ns: Option<i64>,
}

/// The pacing state: the slot the last frame started in, what the display
/// said about recent frames, the level frames run at, and which levels have
/// missed vsyncs before.
pub(crate) struct FramePacer {
    started_slot_ns: Option<i64>,
    depths: Vec<u32>,
    last_shown_ns: Option<i64>,
    last_skip_ns: Option<i64>,
    last_extra_ns: Option<i64>,
    misses: Vec<bool>,
    stage: Option<Stage>,
    holds_ns: [i64; 2],
    failed: [bool; 2],
}

impl Default for FramePacer {
    fn default() -> Self {
        Self {
            started_slot_ns: None,
            depths: Vec::with_capacity(FULL_HISTORY),
            last_shown_ns: None,
            last_skip_ns: None,
            last_extra_ns: None,
            misses: Vec::with_capacity(MISS_WINDOW),
            stage: None,
            holds_ns: [FIRST_HOLD_NS; 2],
            failed: [false; 2],
        }
    }
}

impl FramePacer {
    /// Records what the display said about a frame: when it was shown, in
    /// nanoseconds on the clock frames start on, how many later frames were
    /// queued behind it then, and the display's refresh period.
    pub(crate) fn record_shown(&mut self, shown_ns: i64, queued_behind: u32, vsync_period_ns: i64) {
        if self.depths.len() == FULL_HISTORY {
            self.depths.remove(0);
        }
        self.depths.push(queued_behind);
        let gap = self.last_shown_ns.map(|previous| shown_ns - previous);
        self.last_shown_ns = Some(shown_ns);
        let Some(stage) = self.stage else {
            self.enter(Level::Unpaced, shown_ns, None);
            return;
        };
        if self.misses.len() == MISS_WINDOW {
            self.misses.remove(0);
        }
        self.misses.push(
            gap.is_some_and(|gap| gap * 2 > vsync_period_ns * 3 && gap <= vsync_period_ns * 3),
        );
        let missed = self.misses.iter().filter(|missed| **missed).count();
        if stage
            .level
            .misses_to_rise(self.retrying(stage, shown_ns))
            .is_some_and(|threshold| missed >= threshold)
        {
            self.rise(stage, shown_ns);
        }
    }

    /// Whether `stage` is a try again at a level that missed vsyncs before,
    /// not yet long enough ago to count as working.
    fn retrying(&self, stage: Stage, now_ns: i64) -> bool {
        let failed_before = stage
            .level
            .failure_slot()
            .is_some_and(|slot| self.failed[slot]);
        failed_before && now_ns - stage.since_ns < STABLE_LEVEL_NS
    }

    /// Moves up from `stage`'s level. The level above holds as long as it
    /// did last time: four times that when the level left was being tried
    /// again and failed soon, the first hold when it had worked for a while.
    fn rise(&mut self, stage: Stage, now_ns: i64) {
        let retried_soon = self.retrying(stage, now_ns);
        if let Some(slot) = stage.level.failure_slot() {
            self.failed[slot] = true;
        }
        let above = stage.level.above();
        let hold = match above {
            Level::Shallow => return,
            Level::Buffered => &mut self.holds_ns[0],
            Level::Unpaced => &mut self.holds_ns[1],
        };
        if now_ns - stage.since_ns >= STABLE_LEVEL_NS {
            *hold = FIRST_HOLD_NS;
        } else if retried_soon {
            *hold = (*hold * 4).min(LONGEST_HOLD_NS);
        }
        let until_ns = now_ns + *hold;
        self.enter(above, now_ns, Some(until_ns));
    }

    fn enter(&mut self, level: Level, now_ns: i64, until_ns: Option<i64>) {
        self.stage = Some(Stage {
            level,
            since_ns: now_ns,
            until_ns,
        });
        self.misses.clear();
    }

    /// The level frames run at now, trying the level below once the hold
    /// is over: from unpaced only once the queue has filled, since a loop
    /// that leaves it shallow has nothing to gain from pacing.
    fn level(&mut self, now_ns: i64) -> Option<Level> {
        let stage = self.stage?;
        let hold_over = stage.until_ns.is_none_or(|until| now_ns >= until);
        let step_down = match stage.level {
            Level::Unpaced => {
                hold_over
                    && Level::Buffered
                        .depth()
                        .is_some_and(|depth| self.queue_deeper_than(depth, FULL_HISTORY))
            }
            Level::Buffered => stage.until_ns.is_some() && hold_over,
            Level::Shallow => false,
        };
        if !step_down {
            return Some(stage.level);
        }
        let below = stage.level.below();
        let until_ns = (below == Level::Buffered).then(|| now_ns + self.holds_ns[0]);
        self.enter(below, now_ns, until_ns);
        Some(below)
    }

    /// Whether the last `frames` frames all had more than `depth` queued
    /// behind them.
    fn queue_deeper_than(&self, depth: u32, frames: usize) -> bool {
        self.depths.len() >= frames
            && self.depths[self.depths.len() - frames..]
                .iter()
                .all(|queued| *queued > depth)
    }

    /// Whether a frame may start now. `now_ns` is on the clock the display
    /// reports on, `vsync_ns` a recent vsync's timestamp (`0` when none is
    /// known) and `vsync_period_ns` the display's refresh period.
    ///
    /// Starting a frame takes the current slot; so does giving it up to drain
    /// the queue. Either way the next frame waits for the next vsync, unless
    /// the queue is meant to hold two and holds fewer. An unpaced loop starts
    /// every frame that is due and only notes the slot.
    pub(crate) fn begin_frame(&mut self, now_ns: i64, vsync_ns: i64, vsync_period_ns: i64) -> bool {
        let depth = self.level(now_ns).and_then(Level::depth);
        let slot_ns = self.open_slot(now_ns, vsync_ns, vsync_period_ns);
        let Some(depth) = depth else {
            self.started_slot_ns = slot_ns.or(self.started_slot_ns);
            return true;
        };
        if vsync_ns <= 0 || vsync_period_ns <= 0 {
            return true;
        }
        let Some(slot_ns) = slot_ns else {
            return self.start_extra(depth, now_ns, vsync_period_ns);
        };
        self.started_slot_ns = Some(slot_ns);
        !self.give_up_slot(depth, now_ns, vsync_period_ns)
    }

    /// Whether the current vsync slot has no frame started in it yet, so a
    /// frame that is due should start now rather than wait for the next
    /// vsync.
    pub(crate) fn slot_open(&self, now_ns: i64, vsync_ns: i64, vsync_period_ns: i64) -> bool {
        self.open_slot(now_ns, vsync_ns, vsync_period_ns).is_some()
    }

    /// The current slot's vsync, unless a frame already started in it or no
    /// vsync is known.
    fn open_slot(&self, now_ns: i64, vsync_ns: i64, vsync_period_ns: i64) -> Option<i64> {
        if vsync_ns <= 0 || vsync_period_ns <= 0 {
            return None;
        }
        let elapsed = (now_ns + SLOT_TOLERANCE_NS - vsync_ns).max(0);
        let slot_ns = vsync_ns + elapsed / vsync_period_ns * vsync_period_ns;
        let taken = self
            .started_slot_ns
            .is_some_and(|started| slot_ns - started < vsync_period_ns / 2);
        (!taken).then_some(slot_ns)
    }

    /// Lets a second frame start in a taken slot when the queue is meant to
    /// hold more than one and the display last reported it holding fewer.
    /// One extra frame at a time: the display reports it only a few vsyncs
    /// later.
    fn start_extra(&mut self, depth: u32, now_ns: i64, vsync_period_ns: i64) -> bool {
        let shallower = self.depths.last().is_some_and(|last| *last < depth);
        let settled = self
            .last_extra_ns
            .is_none_or(|extra| now_ns - extra > vsync_period_ns * SHOWN_FRESH_VSYNCS);
        if depth < 2 || !shallower || !settled || !self.shown_recently(now_ns, vsync_period_ns) {
            return false;
        }
        self.last_extra_ns = Some(now_ns);
        true
    }

    /// Gives up the current slot when recent frames all had more queued
    /// behind them than `depth`.
    fn give_up_slot(&mut self, depth: u32, now_ns: i64, vsync_period_ns: i64) -> bool {
        let skipped_recently = self
            .last_skip_ns
            .is_some_and(|skipped_at| now_ns - skipped_at < SKIP_SPACING_NS);
        if skipped_recently
            || !self.queue_deeper_than(depth, HISTORY)
            || !self.shown_recently(now_ns, vsync_period_ns)
        {
            return false;
        }
        self.last_skip_ns = Some(now_ns);
        self.depths.clear();
        true
    }

    fn shown_recently(&self, now_ns: i64, vsync_period_ns: i64) -> bool {
        self.last_shown_ns
            .is_some_and(|shown| now_ns - shown <= vsync_period_ns * SHOWN_FRESH_VSYNCS)
    }
}

#[cfg(test)]
#[path = "tests/frame_pacer_tests.rs"]
mod tests;
