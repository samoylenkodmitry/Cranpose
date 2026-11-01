//! One-time caching of runtime CPU feature probes.
//!
//! `is_x86_feature_detected!` is cheap but not free, and the kernels that
//! consult it sit on per-sample hot paths. Each dispatch site keeps one
//! [`FeatureCache`] in a `static` and asks it once; the answer cannot change
//! while the process runs, so a relaxed atomic is enough.

use std::sync::atomic::{AtomicU8, Ordering};

const UNKNOWN: u8 = 0;
const ON: u8 = 1;
const OFF: u8 = 2;

/// Remembers the result of a boolean probe after its first evaluation.
pub(crate) struct FeatureCache(AtomicU8);

impl FeatureCache {
    pub(crate) const fn new() -> Self {
        FeatureCache(AtomicU8::new(UNKNOWN))
    }

    /// Returns the cached answer, running `probe` only the first time. Two
    /// threads racing on the first call both run the probe and store the
    /// same value, which is harmless.
    #[inline(always)]
    pub(crate) fn get(&self, probe: impl FnOnce() -> bool) -> bool {
        match self.0.load(Ordering::Relaxed) {
            ON => true,
            OFF => false,
            _ => {
                let on = probe();
                self.0.store(if on { ON } else { OFF }, Ordering::Relaxed);
                on
            }
        }
    }
}
