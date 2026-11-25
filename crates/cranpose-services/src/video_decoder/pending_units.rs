//! The access units that a decoder holds, which find the metadata of each decoded picture.

use std::{collections::VecDeque, time::Duration};

use web_time::Instant;

use super::FrameInfo;

// The most units kept. A decoder with low latency holds a few, so a unit older than this many
// never comes out.
const CAPACITY: usize = 64;

// An access unit that the decoder took and has not yet output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingUnit {
    // The timestamp in microseconds, which the decoder returns with the picture
    pub(super) pts_us: u64,
    pub(super) timestamp: Duration,
    pub(super) metadata: u64,
    pub(super) submitted: Instant,
}

impl PendingUnit {
    // Builds a picture's `FrameInfo` from its matching submitted unit, or falls back to `pts_us`
    // when the decoder emitted a picture with no matching submission record.
    #[cfg(any(
        test,
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn into_frame_info(unit: Option<Self>, pts_us: u64) -> FrameInfo {
        unit.map_or_else(
            || FrameInfo {
                timestamp: Duration::from_micros(pts_us),
                metadata: 0,
                decode_time: Duration::ZERO,
            },
            |unit| FrameInfo {
                timestamp: unit.timestamp,
                metadata: unit.metadata,
                decode_time: unit.submitted.elapsed(),
            },
        )
    }
}

// The units in the order of their timestamps, oldest first.
pub(super) struct PendingUnits {
    units: VecDeque<PendingUnit>,
}

impl PendingUnits {
    pub(super) fn new() -> Self {
        Self {
            units: VecDeque::with_capacity(CAPACITY),
        }
    }

    // Adds a unit whose timestamp rises above the units before. A full list drops its oldest
    // unit.
    pub(super) fn push(&mut self, unit: PendingUnit) {
        if self.units.len() == CAPACITY {
            self.units.pop_front();
        }
        self.units.push_back(unit);
    }

    // Takes the unit that the picture at `pts_us` came from. The units before it produced no
    // picture, such as a unit of parameter sets, so they go too. A timestamp that no unit has
    // takes nothing.
    #[cfg(any(
        test,
        target_os = "android",
        target_os = "windows",
        target_os = "linux",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn take(&mut self, pts_us: u64) -> Option<PendingUnit> {
        let index = self.units.iter().position(|unit| unit.pts_us == pts_us)?;
        self.units.drain(..index);
        self.units.pop_front()
    }

    // Takes the unit at `pts_us` and leaves the others, for a decoder that answers each unit
    // alone and may answer out of order (including B-frame presentation reordering).
    #[cfg(any(
        test,
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn remove(&mut self, pts_us: u64) -> Option<PendingUnit> {
        let index = self.units.iter().position(|unit| unit.pts_us == pts_us)?;
        self.units.remove(index)
    }

    // Discards all pending units on flush.
    #[cfg(any(
        test,
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "linux",
        all(feature = "video-web", target_arch = "wasm32", target_os = "unknown")
    ))]
    pub(super) fn clear(&mut self) {
        self.units.clear();
    }

    // The units that wait for their pictures.
    #[cfg(any(test, target_os = "macos", target_os = "ios"))]
    pub(super) fn len(&self) -> usize {
        self.units.len()
    }
}

#[cfg(test)]
#[path = "tests/pending_units_tests.rs"]
mod tests;
