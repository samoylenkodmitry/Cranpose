//! When a layer that presents off the main thread lets one refresh period
//! pass before it takes its next drawable.
//!
//! A drawable presented in time for the window server's next composite
//! reaches the display a fixed number of refresh periods after its present:
//! two on the 60 Hz display of an Apple M3. A thread that presents one
//! drawable each period keeps the queue it started with. When its first
//! frames came faster than the display took them, one drawable more waits in
//! Core Animation's queue for good. Each frame then reaches the display a
//! period later, and the window server spent about three times the CPU on
//! each frame (8.0 against 2.8 ms a frame of a busy Cranpose screen on an
//! Apple M3). [`QueueDepth`] sees that in the time each drawable took to
//! reach the display, and has the next acquire wait one period, which the
//! queue gives back. A wait that gives nothing back, as one while a launch
//! settles or one on a display whose own delay is longer, makes the layer
//! rest a number of shows before it counts late drawables again, twice as
//! many after each such wait. A layer that presents on the main thread waits
//! on its event loop between frames and never waits here.

use alloc::sync::Arc;
use core::{ptr::NonNull, time::Duration};

use block2::RcBlock;
use objc2::{runtime::ProtocolObject, sel};
use objc2_foundation::NSObjectProtocol as _;
use objc2_metal::MTLDrawable;
use parking_lot::Mutex;
use std::sync::OnceLock;

/// Presents in a row that must reach the display late before the next
/// acquire waits, and that must still reach it late after a wait for the
/// wait to count as one that gave nothing back.
const LATE_IN_A_ROW: u8 = 3;
/// The refresh periods from a present to the display that count as late.
const LATE_PERIODS: u64 = 3;
/// The shows a layer rests after its first wait that gave nothing back.
const FIRST_REST: u32 = 60;
/// The most shows a layer rests: about a minute at 60 Hz.
const MOST_REST: u32 = 3840;
/// The intervals between shows the refresh period is the shortest of.
const INTERVALS: usize = 8;
/// The presents kept until their drawables reach the display, by drawable ID.
const PRESENTS: usize = 8;

/// A drawable presented and not shown yet: its ID and when it was presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Present {
    id: u64,
    at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Counts the presents in a row that reached the display late.
    Watching { late: u8 },
    /// The next acquire waits one refresh period.
    Due,
    /// An acquire waited at `since_ns`: the presents after it tell whether
    /// the queue got shorter.
    Checking { since_ns: u64, late: u8 },
    /// A wait gave nothing back: `shows` more shows pass before late
    /// drawables count again.
    Resting { shows: u32 },
}

/// How long a layer's drawables take from their present to the display, and
/// whether its next acquire waits a refresh period. Times are host
/// nanoseconds.
#[derive(Debug)]
pub(super) struct QueueDepth {
    presents: [Option<Present>; PRESENTS],
    last_present_ns: Option<u64>,
    last_shown_ns: Option<u64>,
    intervals: [Option<u64>; INTERVALS],
    next_interval: usize,
    // The rest the next wait that gives nothing back earns.
    next_rest: u32,
    state: State,
}

impl Default for QueueDepth {
    fn default() -> Self {
        Self {
            presents: [None; PRESENTS],
            last_present_ns: None,
            last_shown_ns: None,
            intervals: [None; INTERVALS],
            next_interval: 0,
            next_rest: FIRST_REST,
            state: State::Watching { late: 0 },
        }
    }
}

impl QueueDepth {
    /// Notes that the drawable `id` was presented at `at_ns`.
    pub(super) fn presented(&mut self, id: u64, at_ns: u64) {
        self.presents[slot(id)] = Some(Present { id, at_ns });
        self.last_present_ns = Some(at_ns);
    }

    /// Notes that the drawable `id` reached the display at `shown_ns`, or
    /// never did (`None`).
    pub(super) fn shown(&mut self, id: u64, shown_ns: Option<u64>) {
        let present = self.presents[slot(id)].take_if(|present| present.id == id);
        let (Some(present), Some(shown_ns)) = (present, shown_ns) else {
            return;
        };
        self.note_show(shown_ns);
        let Some(period) = self.period() else {
            return;
        };
        let late = shown_ns.saturating_sub(present.at_ns) / period >= LATE_PERIODS;
        self.state = match self.state {
            State::Watching { late: count } if late => late_again(count),
            State::Watching { .. } => State::Watching { late: 0 },
            State::Due => State::Due,
            State::Checking { since_ns, late: count } if present.at_ns < since_ns => {
                State::Checking { since_ns, late: count }
            }
            State::Checking { .. } if !late => {
                self.next_rest = FIRST_REST;
                State::Watching { late: 0 }
            }
            State::Checking { since_ns, late: count } if count + 1 < LATE_IN_A_ROW => {
                State::Checking { since_ns, late: count + 1 }
            }
            State::Checking { .. } => {
                let shows = self.next_rest;
                self.next_rest = (shows * 2).min(MOST_REST);
                State::Resting { shows }
            }
            State::Resting { shows } if shows > 1 => State::Resting { shows: shows - 1 },
            State::Resting { .. } => State::Watching { late: 0 },
        };
    }

    /// The time the acquire at `now_ns` waits before it takes a drawable. A
    /// layer that presented nothing for two periods has an empty queue, and
    /// its next frame waits for nothing.
    pub(super) fn wait_before_acquire(&mut self, now_ns: u64) -> Option<Duration> {
        if self.state != State::Due {
            return None;
        }
        let period = self.period()?;
        let idle = self
            .last_present_ns
            .is_none_or(|at_ns| now_ns.saturating_sub(at_ns) > 2 * period);
        if idle {
            self.state = State::Watching { late: 0 };
            return None;
        }
        self.state = State::Checking {
            since_ns: now_ns,
            late: 0,
        };
        Some(Duration::from_nanos(period))
    }

    fn note_show(&mut self, shown_ns: u64) {
        if let Some(last) = self.last_shown_ns.filter(|&last| shown_ns > last) {
            self.intervals[self.next_interval] = Some(shown_ns - last);
            self.next_interval = (self.next_interval + 1) % INTERVALS;
        }
        self.last_shown_ns = Some(self.last_shown_ns.map_or(shown_ns, |last| last.max(shown_ns)));
    }

    /// The display's refresh period: the shortest of the recent intervals
    /// between shows.
    fn period(&self) -> Option<u64> {
        self.intervals.iter().flatten().copied().min()
    }
}

fn late_again(count: u8) -> State {
    if count + 1 < LATE_IN_A_ROW {
        State::Watching { late: count + 1 }
    } else {
        State::Due
    }
}

fn slot(id: u64) -> usize {
    (id % PRESENTS as u64) as usize
}

/// What Core Animation calls as each drawable reaches the display.
type ShownHandler = RcBlock<dyn Fn(NonNull<ProtocolObject<dyn MTLDrawable>>)>;

/// A layer's [`QueueDepth`], fed by its presents off the main thread and by
/// Core Animation's word of when each of those drawables reached the
/// display.
#[derive(Debug)]
pub(super) struct PresentQueue {
    depth: Arc<Mutex<QueueDepth>>,
    // Core Animation calls it as each drawable reaches the display. It is
    // made once: a present hands Core Animation this block, not a new one.
    on_shown: ShownHandler,
    // Whether the layer's drawables say when they reach the display, which
    // the simulator's do not.
    tells_shows: OnceLock<bool>,
    timer: super::time::PresentationTimer,
}

impl PresentQueue {
    pub(super) fn new() -> Self {
        let depth = Arc::new(Mutex::new(QueueDepth::default()));
        let shown_depth = Arc::clone(&depth);
        let on_shown = RcBlock::new(move |drawable: NonNull<ProtocolObject<dyn MTLDrawable>>| {
            let drawable = unsafe { drawable.as_ref() };
            // Host time in seconds; 0 for a drawable that never reached the
            // display.
            let shown: f64 = unsafe { objc2::msg_send![drawable, presentedTime] };
            let shown_ns = (shown > 0.0).then_some((shown * 1e9) as u64);
            shown_depth
                .lock()
                .shown(drawable.drawableID() as u64, shown_ns);
        });
        Self {
            depth,
            on_shown,
            tells_shows: OnceLock::new(),
            timer: super::time::PresentationTimer::new(),
        }
    }

    /// Notes a present of `drawable` made off the main thread, and asks Core
    /// Animation to say when it reaches the display.
    pub(super) fn presenting(&self, drawable: &ProtocolObject<dyn MTLDrawable>) {
        if objc2::MainThreadMarker::new().is_some() {
            return;
        }
        let tells_shows = *self.tells_shows.get_or_init(|| {
            drawable.respondsToSelector(sel!(addPresentedHandler:))
                && drawable.respondsToSelector(sel!(presentedTime))
        });
        if !tells_shows {
            return;
        }
        self.depth
            .lock()
            .presented(drawable.drawableID() as u64, self.now_ns());
        unsafe { drawable.addPresentedHandler(RcBlock::as_ptr(&self.on_shown)) };
    }

    /// Off the main thread, waits as long as the layer's queue asks of the
    /// acquire about to take a drawable.
    pub(super) fn before_acquire(&self) {
        if objc2::MainThreadMarker::new().is_some() {
            return;
        }
        let wait = self.depth.lock().wait_before_acquire(self.now_ns());
        if let Some(wait) = wait {
            #[expect(
                clippy::disallowed_methods,
                reason = "a render thread off the main thread waits, as nextDrawable would"
            )]
            std::thread::sleep(wait);
        }
    }

    /// Starts over for a new configuration, which may come with another
    /// display and another refresh period.
    pub(super) fn reconfigured(&self) {
        *self.depth.lock() = QueueDepth::default();
    }

    fn now_ns(&self) -> u64 {
        u64::try_from(self.timer.get_timestamp_ns()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
#[path = "tests/present_queue_tests.rs"]
mod tests;
