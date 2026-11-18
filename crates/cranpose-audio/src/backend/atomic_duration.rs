//! A duration that one thread publishes and another reads, with no lock. The audio callback of a
//! device writes it, so a write may not block.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

// The word of a duration that no thread has published. `store` never writes it.
const EMPTY: u64 = u64::MAX;

pub(super) struct AtomicDuration(AtomicU64);

impl AtomicDuration {
    pub(super) const fn new() -> Self {
        Self(AtomicU64::new(EMPTY))
    }

    // Publishes `value` at nanosecond precision. A value too long for the word keeps the longest
    // that fits, so it never reads back as empty.
    pub(super) fn store(&self, value: Duration) {
        let nanos = u64::try_from(value.as_nanos())
            .unwrap_or(EMPTY)
            .min(EMPTY - 1);
        self.0.store(nanos, Ordering::Relaxed);
    }

    // The latest published value, or `None` before the first.
    pub(super) fn load(&self) -> Option<Duration> {
        match self.0.load(Ordering::Relaxed) {
            EMPTY => None,
            nanos => Some(Duration::from_nanos(nanos)),
        }
    }
}
