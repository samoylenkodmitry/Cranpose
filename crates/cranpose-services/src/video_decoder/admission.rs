//! The checks that every decoder makes of an access unit before it takes the unit.

use super::{AccessUnit, DecoderError};

// The order and the size that a decoder takes units in.
pub(super) struct Admission {
    last_pts_us: Option<u64>,
    // The largest access unit that the decoder takes
    limit: usize,
}

impl Admission {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            last_pts_us: None,
            limit,
        }
    }

    // The unit's presentation time in microseconds, if the decoder takes the unit: its time rises
    // above the time of the unit before, and it fits the limit.
    pub(super) fn check(&self, unit: &AccessUnit<'_>) -> Result<u64, DecoderError> {
        let pts_us = u64::try_from(unit.timestamp.as_micros()).unwrap_or(u64::MAX);
        if self.last_pts_us.is_some_and(|last| pts_us <= last) {
            return Err(DecoderError::TimestampNotRising);
        }
        if unit.data.len() > self.limit {
            return Err(DecoderError::AccessUnitTooLarge {
                size: unit.data.len(),
                limit: self.limit,
            });
        }
        Ok(pts_us)
    }

    // Records the unit at `pts_us` as taken.
    pub(super) fn accept(&mut self, pts_us: u64) {
        self.last_pts_us = Some(pts_us);
    }
}
