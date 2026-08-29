use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct CostTuner {
    name: &'static str,
    min_entries: usize,
    cheap_ns: u64,
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
        if serial == 0 {
            return false;
        }
        if serial.saturating_mul(entries as u64) < self.cheap_ns {
            return false;
        }
        if parallel == 0 {
            return true;
        }
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

        assert!(!tuner.choose_parallel(99));

        assert!(!tuner.choose_parallel(1000));
        tuner.record(false, 1000, 10_000_000);

        assert!(tuner.choose_parallel(1000));
        tuner.record(true, 1000, 2_000_000);

        let mut parallel_wins = 0;
        for _ in 0..100 {
            if tuner.choose_parallel(1000) {
                parallel_wins += 1;
            }
        }
        assert!(parallel_wins >= 99);

        let cheap = CostTuner::new("test", 100, 1_000_000_000);
        cheap.record(false, 1000, 1_000);
        assert!(!cheap.choose_parallel(1000));
    }
}
