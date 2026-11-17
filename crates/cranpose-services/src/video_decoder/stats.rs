//! The counts and times that a decoder reports.

use std::time::Duration;

/// What a decoder has done since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecoderStats {
    /// The access units that the decoder took.
    pub submitted: u64,
    /// The pictures that the decoder output.
    pub decoded: u64,
    /// The pictures that the decoder dropped because the app did not take them in time.
    pub dropped: u64,
    /// The pictures that reached the app's reader after the next picture began to decode.
    pub late: u64,
    /// The decode time of the latest picture.
    pub last_decode: Duration,
    /// The decode time of the pictures so far, as a moving average.
    pub mean_decode: Duration,
    /// The longest decode time.
    pub max_decode: Duration,
}

#[cfg(target_os = "android")]
pub(super) use counters::StatsCounters;

#[cfg(target_os = "android")]
mod counters {
    use std::{
        sync::atomic::{AtomicU64, Ordering::Relaxed},
        time::Duration,
    };

    use super::DecoderStats;

    // The counters of a decoder. Its threads add to them without a lock.
    #[derive(Default)]
    pub(crate) struct StatsCounters {
        submitted: AtomicU64,
        decoded: AtomicU64,
        dropped: AtomicU64,
        late: AtomicU64,
        last_decode_ns: AtomicU64,
        mean_decode_ns: AtomicU64,
        max_decode_ns: AtomicU64,
    }

    impl StatsCounters {
        pub(crate) fn submitted(&self) {
            self.submitted.fetch_add(1, Relaxed);
        }

        pub(crate) fn dropped(&self) {
            self.dropped.fetch_add(1, Relaxed);
        }

        pub(crate) fn late(&self) {
            self.late.fetch_add(1, Relaxed);
        }

        // Only the output thread decodes, so the average needs no compare and swap.
        pub(crate) fn decoded(&self, decode_time: Duration) {
            let ns = u64::try_from(decode_time.as_nanos()).unwrap_or(u64::MAX);
            let count = self.decoded.fetch_add(1, Relaxed);
            let mean = self.mean_decode_ns.load(Relaxed);
            let mean = if count == 0 {
                ns
            } else {
                mean.saturating_add(ns / 8).saturating_sub(mean / 8)
            };
            self.mean_decode_ns.store(mean, Relaxed);
            self.last_decode_ns.store(ns, Relaxed);
            self.max_decode_ns.fetch_max(ns, Relaxed);
        }

        pub(crate) fn snapshot(&self) -> DecoderStats {
            let time = |ns: &AtomicU64| Duration::from_nanos(ns.load(Relaxed));
            DecoderStats {
                submitted: self.submitted.load(Relaxed),
                decoded: self.decoded.load(Relaxed),
                dropped: self.dropped.load(Relaxed),
                late: self.late.load(Relaxed),
                last_decode: time(&self.last_decode_ns),
                mean_decode: time(&self.mean_decode_ns),
                max_decode: time(&self.max_decode_ns),
            }
        }
    }
}
