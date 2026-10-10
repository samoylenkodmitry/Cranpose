//! When a command pool gives its memory back to the driver.
//!
//! A pool reset without `RELEASE_RESOURCES` keeps the memory its command
//! buffers took, so the next recording reuses it. A tile-based driver takes
//! command memory for each render pass, so a pool that recorded a burst of
//! heavy frames would keep the burst's memory for as long as it lives.
//! [`PoolTrim`] gives it back once the burst is over; a steady workload, light
//! or heavy, keeps its memory and records as fast as it did.

/// The render passes a pool must have held for its memory to be worth giving
/// back.
const HEAVY_PASSES: u32 = 8;
/// The resets in a row that must each use less than half of the pool's peak
/// before it gives its memory back.
const LIGHT_RESETS: u8 = 4;

/// What one command pool recorded, for when its reset releases its memory.
#[derive(Debug, Default)]
pub(super) struct PoolTrim {
    /// Render passes begun since the last reset.
    passes: u32,
    /// The most passes a reset saw since the pool last gave its memory back.
    peak: u32,
    /// Resets in a row that used less than half of `peak`.
    light: u8,
}

impl PoolTrim {
    pub(super) fn begin_render_pass(&mut self) {
        self.passes = self.passes.saturating_add(1);
    }

    /// Whether this reset gives the pool's memory back: it held at least
    /// [`HEAVY_PASSES`] render passes, and this is the [`LIGHT_RESETS`]th reset
    /// in a row to use less than half of that.
    pub(super) fn reset(&mut self) -> bool {
        let used = core::mem::take(&mut self.passes);
        if self.peak < HEAVY_PASSES || used.saturating_mul(2) >= self.peak {
            self.light = 0;
            self.peak = self.peak.max(used);
            return false;
        }
        self.light += 1;
        if self.light < LIGHT_RESETS {
            return false;
        }
        self.light = 0;
        self.peak = used;
        true
    }
}

#[cfg(test)]
#[path = "tests/pool_trim_tests.rs"]
mod tests;
