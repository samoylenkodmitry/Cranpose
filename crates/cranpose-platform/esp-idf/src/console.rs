#![expect(unsafe_code, reason = "calls the console writer in C")]

// The lines the frame loop writes to the console, in ESP-IDF's log format,
// through `components/cranpose_console/console.c`: a line goes out only when
// the console's FIFO takes it whole at once, so the loop never waits on a
// host that reads nothing. A line the console cannot take is the caller's to
// keep for a later frame, or is dropped and counted.

use std::{
    fmt::{self, Write},
    sync::atomic::{AtomicU32, Ordering},
};

use esp_idf_svc::sys;

// The most bytes the console takes at once: the USB Serial/JTAG's packet.
// A UART's FIFO takes 128.
const LINE_BYTES: usize = 64;

static DROPPED: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    fn cranpose_console_write_line(line: *const u8, len: usize) -> bool;
}

// A line in its stack buffer; text past its end fails the write.
struct Line {
    bytes: [u8; LINE_BYTES],
    len: usize,
}

impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len + text.len();
        let room = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        room.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

// The time a line names: ESP-IDF's log clock, in milliseconds.
#[derive(Clone, Copy)]
pub(crate) struct Stamp(u32);

impl Stamp {
    pub(crate) fn now() -> Self {
        // SAFETY: reads the log clock, which any task may read.
        Self(unsafe { sys::esp_log_timestamp() })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Level {
    Info,
    Warn,
}

// What became of a line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sent {
    // It went out, or it is longer than `LINE_BYTES` and was dropped and
    // counted, as no console would ever take it.
    Done,
    // The console cannot take it now: the caller may keep it for later.
    Busy,
}

// Writes `I (STAMP) cranpose: TEXT` when the console takes the whole line
// now. Out of line: one copy serves every line.
#[inline(never)]
pub(crate) fn write(stamp: Stamp, level: Level, text: fmt::Arguments<'_>) -> Sent {
    let mut line = Line {
        bytes: [0; LINE_BYTES],
        len: 0,
    };
    let letter = match level {
        Level::Info => 'I',
        Level::Warn => 'W',
    };
    if write!(line, "{letter} ({}) cranpose: {text}\r\n", stamp.0).is_err() {
        count_dropped(1);
        return Sent::Done;
    }
    // SAFETY: the writer reads `len` bytes of the buffer and keeps no
    // pointer to it.
    if unsafe { cranpose_console_write_line(line.bytes.as_ptr(), line.len) } {
        Sent::Done
    } else {
        Sent::Busy
    }
}

// Writes the line now, or drops it and counts the drop.
pub(crate) fn write_or_drop(level: Level, text: fmt::Arguments<'_>) {
    if write(Stamp::now(), level, text) == Sent::Busy {
        count_dropped(1);
    }
}

pub(crate) fn count_dropped(lines: u32) {
    DROPPED.fetch_add(lines, Ordering::Relaxed);
}

pub(crate) fn dropped() -> u32 {
    DROPPED.load(Ordering::Relaxed)
}

// A count of hundredths, written with two decimals.
pub(crate) struct Hundredths(pub(crate) u32);

impl fmt::Display for Hundredths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

// A count of tenths, written with one decimal.
pub(crate) struct Tenths(pub(crate) u32);

impl fmt::Display for Tenths {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.0 / 10, self.0 % 10)
    }
}
