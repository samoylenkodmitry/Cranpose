//! A tap on the mixed output that reports the level of each ear once a second.

use std::sync::atomic::{AtomicBool, Ordering};

use super::seq_cell::SeqCell;

/// The levels of the mix over one second of output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputLevels {
    /// Which second this is, counted from 1 since the tap was made. A reader that sees the same
    /// number again has seen these levels before.
    pub window: u32,
    /// The root mean square of the left channel over the second.
    pub left: f32,
    /// The root mean square of the right channel over the second.
    pub right: f32,
}

/// A debug tap on the mixed output. The mixer sums the squares of the left and right samples
/// that it writes and publishes the root mean square of each, once for every second of output.
/// The tap is off until [`set_enabled`](Self::set_enabled) turns it on. Off, the mixer does no
/// work for it beyond one read of a flag in each render call.
///
/// A reader polls [`levels`](Self::levels) from any thread and logs what it finds. The audio
/// thread never logs.
pub struct OutputTap {
    enabled: AtomicBool,
    // The window number, the left level and the right level
    window: SeqCell<3>,
}

impl Default for OutputTap {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputTap {
    /// A tap that is off and has no levels yet.
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            window: SeqCell::new([0; 3]),
        }
    }

    /// Turns the tap on or off.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the tap is on. A mixer reads it before each render call.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// The levels of the last whole second that the mixer wrote with the tap on, or `None` while
    /// there is none yet or a write overlaps the read.
    pub fn levels(&self) -> Option<OutputLevels> {
        let [window, left, right] = self.window.read()?;
        (window != 0).then(|| OutputLevels {
            window,
            left: f32::from_bits(left),
            right: f32::from_bits(right),
        })
    }

    /// Publishes the levels of the next second. Only the mixer calls it.
    pub fn publish(&self, left: f32, right: f32) {
        let window = self.window.read().map_or(0, |[window, _, _]| window);
        self.window.write([
            window.wrapping_add(1).max(1),
            left.to_bits(),
            right.to_bits(),
        ]);
    }
}
