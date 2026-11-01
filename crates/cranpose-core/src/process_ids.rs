//! Numbers that never repeat in a process, taken from any thread.

#[cfg(target_has_atomic = "64")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_has_atomic = "64"))]
use std::sync::{Mutex, PoisonError};

/// Hands out numbers that never repeat in a process, from any thread: the
/// ids of observers, listeners and other registrations.
///
/// A target without 64-bit atomics, such as the ESP32's Xtensa cores, takes
/// them under a lock: portable-atomic's 64-bit atomics there keep a table of
/// 67 locks, 4,288 bytes of RAM.
pub struct ProcessIds {
    #[cfg(target_has_atomic = "64")]
    next: AtomicU64,
    #[cfg(not(target_has_atomic = "64"))]
    next: Mutex<u64>,
}

impl ProcessIds {
    /// Ids from `first` on.
    pub const fn starting_at(first: u64) -> Self {
        Self {
            #[cfg(target_has_atomic = "64")]
            next: AtomicU64::new(first),
            #[cfg(not(target_has_atomic = "64"))]
            next: Mutex::new(first),
        }
    }

    /// An id no earlier call returned.
    pub fn next(&self) -> u64 {
        #[cfg(target_has_atomic = "64")]
        {
            self.next.fetch_add(1, Ordering::Relaxed)
        }
        #[cfg(not(target_has_atomic = "64"))]
        {
            let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            let id = *next;
            *next += 1;
            id
        }
    }
}
