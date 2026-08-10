//! Measured serial-vs-parallel switching for per-frame fan-out stages.
//!
//! A scoped-thread spawn wave has a fixed wall-clock price, so fanning a
//! stage out only pays when the work is slow enough to dwarf it — and "slow
//! enough" is a property of the core the code landed on, not of the code. A
//! Kirin 980 big core clears a 15k-shape MEGA emit in ~3 ms and measured
//! WORSE with workers; a watch-class in-order core takes tens of
//! milliseconds for the same run and has three idle siblings. Rather than
//! guess by device, each [`CostTuner`] times both paths where they actually
//! run: every large invocation records its ns-per-entry into an EMA for the
//! path taken, the cheaper EMA wins the next invocation, and every 128th
//! large invocation deliberately runs the loser so a stale verdict (DVFS,
//! thermals, the scheduler moving the thread to a slower core) can be
//! overturned.

use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct CostTuner {
    /// Stage name for the one-shot first-trial log line.
    name: &'static str,
    /// Below this many entries the stage is always serial and untimed.
    min_entries: usize,
    /// Serial invocations projected cheaper than this are not worth a spawn
    /// wave regardless of the EMAs.
    cheap_ns: u64,
    /// EMA of ns per entry for each path; 0 = no sample yet.
    serial_ns: AtomicU64,
    parallel_ns: AtomicU64,
    decisions: AtomicU64,
}

impl CostTuner {
    pub(crate) const fn new(name: &'static str, min_entries: usize, cheap_ns: u64) -> Self {
        Self {
            name,
            min_entries,
            cheap_ns,
            serial_ns: AtomicU64::new(0),
            parallel_ns: AtomicU64::new(0),
            decisions: AtomicU64::new(0),
        }
    }

    pub(crate) fn choose_parallel(&self, entries: usize) -> bool {
        if entries < self.min_entries {
            return false;
        }
        let decision = self.decisions.fetch_add(1, Ordering::Relaxed);
        let serial = self.serial_ns.load(Ordering::Relaxed);
        let parallel = self.parallel_ns.load(Ordering::Relaxed);
        // Bootstrap: sample serial first, then parallel once serial has
        // proven expensive enough to bother.
        if serial == 0 {
            return false;
        }
        if serial.saturating_mul(entries as u64) < self.cheap_ns {
            return false;
        }
        if parallel == 0 {
            return true;
        }
        // Re-trial the losing path occasionally so the verdict can flip.
        if decision.is_multiple_of(128) {
            return parallel >= serial;
        }
        parallel < serial
    }

    pub(crate) fn record(&self, parallel: bool, entries: usize, elapsed_ns: u64) {
        if entries < self.min_entries {
            return;
        }
        let per_entry = (elapsed_ns / entries as u64).max(1);
        let slot = if parallel {
            &self.parallel_ns
        } else {
            &self.serial_ns
        };
        let old = slot.load(Ordering::Relaxed);
        if parallel && old == 0 {
            // One line per process: the verdict of the first head-to-head
            // trial is the number that explains this device's behaviour.
            log::info!(
                "[cost-tuner] {}: first parallel trial {} ns/entry vs serial {} ns/entry",
                self.name,
                per_entry,
                self.serial_ns.load(Ordering::Relaxed),
            );
        }
        let new = if old == 0 {
            per_entry
        } else {
            (old * 7 + per_entry) / 8
        };
        slot.store(new.max(1), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::CostTuner;

    #[test]
    fn tuner_bootstraps_serial_then_settles_on_the_measured_winner() {
        let tuner = CostTuner::new("test", 100, 1_000_000);

        // Small invocations never fan out.
        assert!(!tuner.choose_parallel(99));

        // First large invocation: no serial sample yet -> serial.
        assert!(!tuner.choose_parallel(1000));
        tuner.record(false, 1000, 10_000_000); // 10µs/entry: expensive

        // Serial proven expensive, parallel unsampled -> trial parallel.
        assert!(tuner.choose_parallel(1000));
        tuner.record(true, 1000, 2_000_000); // 2µs/entry: parallel wins

        // Winner sticks (skip index 128 where the loser is re-trialed).
        let mut parallel_wins = 0;
        for _ in 0..100 {
            if tuner.choose_parallel(1000) {
                parallel_wins += 1;
            }
        }
        assert!(parallel_wins >= 99);

        // A cheap workload stays serial even with a parallel-favouring EMA.
        let cheap = CostTuner::new("test", 100, 1_000_000_000);
        cheap.record(false, 1000, 1_000); // 1ns/entry
        assert!(!cheap.choose_parallel(1000));
    }
}
