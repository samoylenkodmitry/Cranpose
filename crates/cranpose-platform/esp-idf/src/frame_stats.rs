// The frame statistics of the loop: the first frame's times, and for every
// window the frames, their average update and draw times, the slowest frame,
// the bytes sent and the memory left. Their lines go to the console when it
// takes them; a window's lines wait until it does, or until the next window
// replaces them.

use std::time::{Duration, Instant};

use crate::{
    console::{self, Hundredths, Level, Stamp, Tenths},
    memory::MemoryReport,
};

#[derive(Clone, Copy)]
pub(crate) struct FramePhases {
    pub(crate) update: Duration,
    pub(crate) present: Duration,
    // The bytes the frame sent to the panel.
    pub(crate) sent: usize,
}

pub(crate) struct FrameStats {
    length: Duration,
    start: Instant,
    window: Option<Window>,
    first: Option<Lines<FirstFrame>>,
    report: Option<Lines<Report>>,
}

struct Window {
    start: Instant,
    frames: u32,
    update: Duration,
    present: Duration,
    slowest: Duration,
    sent: usize,
}

// The lines of `text` from `next` on, not yet written, and the time they
// name.
struct Lines<T> {
    text: T,
    stamp: Stamp,
    next: u8,
}

// The first frame's times, kept until its lines are written: the bench reads
// the first one.
struct FirstFrame {
    after_ms: u32,
    update_ms: u32,
    present_ms: u32,
}

struct Report {
    frames: u32,
    tenths_s: u32,
    update_hundredths_ms: u32,
    present_hundredths_ms: u32,
    slowest_tenths_ms: u32,
    sent_tenths_kb: u32,
    memory: MemoryReport,
}

trait LineText {
    const LINES: u8;

    // Writes line `index` when the console takes it now.
    fn write(&self, stamp: Stamp, index: u8) -> bool;
}

impl LineText for FirstFrame {
    const LINES: u8 = 2;

    fn write(&self, stamp: Stamp, index: u8) -> bool {
        match index {
            0 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "first frame after {} ms: update {} ms",
                    self.after_ms, self.update_ms
                ),
            ),
            _ => console::write(
                stamp,
                Level::Info,
                format_args!("first frame drawn and sent in {} ms", self.present_ms),
            ),
        }
    }
}

impl LineText for Report {
    const LINES: u8 = 6;

    fn write(&self, stamp: Stamp, index: u8) -> bool {
        let memory = &self.memory;
        match index {
            0 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "{} frames in {} s: update {} ms",
                    self.frames,
                    Tenths(self.tenths_s),
                    Hundredths(self.update_hundredths_ms)
                ),
            ),
            1 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "draw and send {} ms",
                    Hundredths(self.present_hundredths_ms)
                ),
            ),
            2 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "slowest {} ms, sent {} KB a frame",
                    Tenths(self.slowest_tenths_ms),
                    Tenths(self.sent_tenths_kb)
                ),
            ),
            3 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "heap {} KB free, {} KB in one block",
                    memory.heap_free / 1024,
                    memory.heap_largest / 1024
                ),
            ),
            4 => console::write(
                stamp,
                Level::Info,
                format_args!(
                    "heap {} KB at least; {} lines dropped",
                    memory.heap_lowest / 1024,
                    console::dropped()
                ),
            ),
            _ => console::write(
                stamp,
                Level::Info,
                format_args!("stack {} bytes never used", memory.stack_unused),
            ),
        }
    }
}

impl<T: LineText> Lines<T> {
    fn new(text: T) -> Self {
        Self {
            text,
            stamp: Stamp::now(),
            next: 0,
        }
    }

    // Writes the lines the console takes now, and returns whether all are
    // written.
    fn write(&mut self) -> bool {
        while self.next < T::LINES {
            if !self.text.write(self.stamp, self.next) {
                return false;
            }
            self.next += 1;
        }
        true
    }
}

// `value` over `count`, in units of `unit_us` microseconds, saturating.
fn per(value: Duration, count: u32, unit_us: u64) -> u32 {
    let micros = value
        .as_secs()
        .saturating_mul(1_000_000)
        .saturating_add(u64::from(value.subsec_micros()));
    u32::try_from(micros / unit_us / u64::from(count.max(1))).unwrap_or(u32::MAX)
}

impl FrameStats {
    // Statistics of windows of `length`, the first frame timed from `start`.
    pub(crate) fn new(length: Duration, start: Instant) -> Self {
        Self {
            length,
            start,
            window: None,
            first: None,
            report: None,
        }
    }

    // Counts a frame presented at `presented`, and writes the waiting lines
    // that the console takes.
    pub(crate) fn record(&mut self, phases: FramePhases, presented: Instant) {
        match self.window.as_mut() {
            Some(window) => {
                window.frames += 1;
                window.update += phases.update;
                window.present += phases.present;
                window.sent += phases.sent;
                window.slowest = window.slowest.max(phases.update + phases.present);
                if presented.duration_since(window.start) >= self.length {
                    self.end_window(presented);
                }
            }
            None => self.first_frame(phases, presented),
        }
        if self.first.is_some() || self.report.is_some() {
            self.write_waiting();
        }
    }

    // Keeps the first frame's times and starts counting after it. Cold, as
    // the reports below: the loop runs every frame, a report seldom.
    #[cold]
    #[inline(never)]
    fn first_frame(&mut self, phases: FramePhases, presented: Instant) {
        self.first = Some(Lines::new(FirstFrame {
            after_ms: per(presented.duration_since(self.start), 1, 1000),
            update_ms: per(phases.update, 1, 1000),
            present_ms: per(phases.present, 1, 1000),
        }));
        self.window = Some(Window::new(presented));
    }

    // Keeps the report of the window that ended at `presented`, in place of
    // one whose lines did not all go out, and starts the next window.
    #[cold]
    #[inline(never)]
    fn end_window(&mut self, presented: Instant) {
        let Some(window) = self.window.replace(Window::new(presented)) else {
            return;
        };
        if let Some(unwritten) = &self.report {
            console::count_dropped(u32::from(Report::LINES - unwritten.next));
        }
        let frames = window.frames;
        let sent = u32::try_from(window.sent).unwrap_or(u32::MAX);
        self.report = Some(Lines::new(Report {
            frames,
            tenths_s: per(presented.duration_since(window.start), 1, 100_000),
            update_hundredths_ms: per(window.update, frames, 10),
            present_hundredths_ms: per(window.present, frames, 10),
            slowest_tenths_ms: per(window.slowest, 1, 100),
            sent_tenths_kb: sent.saturating_mul(10) / 1024 / frames.max(1),
            memory: MemoryReport::now(),
        }));
    }

    #[cold]
    #[inline(never)]
    fn write_waiting(&mut self) {
        if let Some(first) = &mut self.first {
            if !first.write() {
                return;
            }
            self.first = None;
        }
        if let Some(report) = &mut self.report
            && report.write()
        {
            self.report = None;
        }
    }
}

impl Window {
    fn new(start: Instant) -> Self {
        Self {
            start,
            frames: 0,
            update: Duration::ZERO,
            present: Duration::ZERO,
            slowest: Duration::ZERO,
            sent: 0,
        }
    }
}
