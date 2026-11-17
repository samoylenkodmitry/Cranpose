//! What a streamed scene did with its frames.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// How a streamed scene treated the frames that its decoder output, counted since the scene began.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamCounts {
    /// The frames that the headset drew and handed to the compositor.
    pub shown: u64,
    /// The frames that the headset never showed: a closer or newer frame took their place, the
    /// queue of waiting frames was full, the runtime showed none of the app's layers, or the
    /// app's views for the frame were refused.
    pub skipped: u64,
    /// The headset frames that showed the previous frame's picture again, because no waiting
    /// frame was closer to the display time. A 30 Hz stream on a 120 Hz headset repeats 3 of
    /// every 4 frames.
    pub repeated: u64,
}

/// A handle to the counts of a [`StreamedScene`](crate::StreamedScene). An app makes one, gives a
/// clone to the scene, and reads [`counts`](Self::counts) from any thread, for example to report
/// the quality of the stream to its host. Two handles are equal when they share counts.
#[derive(Clone, Debug, Default)]
pub struct StreamStats {
    shared: Arc<Shared>,
}

#[derive(Debug, Default)]
struct Shared {
    shown: AtomicU64,
    skipped: AtomicU64,
    repeated: AtomicU64,
}

impl StreamStats {
    /// A handle that counts nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The counts of the scene that holds this handle, as of the headset's last frame.
    pub fn counts(&self) -> StreamCounts {
        StreamCounts {
            shown: self.shared.shown.load(Ordering::Relaxed),
            skipped: self.shared.skipped.load(Ordering::Relaxed),
            repeated: self.shared.repeated.load(Ordering::Relaxed),
        }
    }

    #[cfg_attr(
        not(any(test, all(headset, target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Replaces the counts with the scene's totals. The frame loop is the only writer, so each
    // field is a plain store.
    pub(crate) fn publish(&self, counts: StreamCounts) {
        self.shared.shown.store(counts.shown, Ordering::Relaxed);
        self.shared.skipped.store(counts.skipped, Ordering::Relaxed);
        self.shared
            .repeated
            .store(counts.repeated, Ordering::Relaxed);
    }
}

impl PartialEq for StreamStats {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

#[cfg(test)]
#[path = "tests/stream_stats_tests.rs"]
mod tests;
