//! The access units that a decoder holds, which find the metadata of each decoded picture.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

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
    pub(super) fn take(&mut self, pts_us: u64) -> Option<PendingUnit> {
        let index = self.units.iter().position(|unit| unit.pts_us == pts_us)?;
        self.units.drain(..index);
        self.units.pop_front()
    }
}

#[cfg(test)]
#[path = "tests/pending_units_tests.rs"]
mod tests;
