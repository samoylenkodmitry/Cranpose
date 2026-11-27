//! A minimal lock-free, write-once boxed cell: the no_std-friendly stand-in for
//! `std::sync::OnceLock` used to cache the runtime-built lookup tables. It needs
//! only an atomic pointer and `alloc`, so the crate stays no_std (with `alloc`)
//! when the `std` feature is off, while behaving the same on std.
//!
//! Targets with a native pointer-width atomic (std, Cortex-M3+, wasm, x86, ...)
//! use `core`'s atomics directly. Targets without one (Cortex-M0/M0+, AVR) fall
//! back to `portable-atomic`, which the consuming binary configures with a
//! critical-section implementation.

use alloc::boxed::Box;
use core::ptr;
#[cfg(target_has_atomic = "ptr")]
use core::sync::atomic::{AtomicPtr, Ordering};
#[cfg(not(target_has_atomic = "ptr"))]
use portable_atomic::{AtomicPtr, Ordering};

/// A cell holding `Box<T>`, built on first access and shared thereafter. Every
/// use in this crate is a `static`, so the value lives for the whole program
/// and its destructor never runs (the same leak-for-program-lifetime semantics
/// as a `static` `OnceLock`). The `Drop` impl below only matters for
/// non-static cells, such as the ones the tests create.
pub struct OnceBox<T> {
    ptr: AtomicPtr<T>,
}

// The cell hands out `&T` across threads, which needs `T: Sync`; the value may
// be built on any thread, which needs `T: Send`.
unsafe impl<T: Send + Sync> Sync for OnceBox<T> {}
unsafe impl<T: Send> Send for OnceBox<T> {}

impl<T> OnceBox<T> {
    /// An empty cell. `const` so it can initialize a `static` directly.
    pub const fn new() -> Self {
        Self {
            ptr: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// The stored value, building it with `init` on the first call. If two
    /// threads race the first call, one stores its box and the other drops its
    /// own; both then read the stored value.
    pub fn get_or_init(&self, init: impl FnOnce() -> Box<T>) -> &T {
        let existing = self.ptr.load(Ordering::Acquire);
        let p = if existing.is_null() {
            let new = Box::into_raw(init());
            match self.ptr.compare_exchange(
                ptr::null_mut(),
                new,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => new,
                Err(winner) => {
                    // Lost the race: reclaim our box, use the winner's pointer.
                    drop(unsafe { Box::from_raw(new) });
                    winner
                }
            }
        } else {
            existing
        };
        // SAFETY: `p` is non-null and points at a `Box<T>` leaked above (by us or
        // the race winner); the value is never freed while the cell lives.
        unsafe { &*p }
    }
}

impl<T> Default for OnceBox<T> {
    /// An empty cell, the same as [`OnceBox::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for OnceBox<T> {
    /// Frees the stored box for a non-static cell. Static cells never reach
    /// here (they live for the whole program), so this only runs for the
    /// short-lived cells the tests build.
    fn drop(&mut self) {
        // Exclusive access in Drop, so no atomic load is needed. Static cells
        // never run Drop (the program-lifetime case), but a non-static cell
        // reclaims its box here rather than leaking it.
        let p = *self.ptr.get_mut();
        if !p.is_null() {
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OnceBox;
    use alloc::boxed::Box;

    /// The first init wins and a later call never replaces the stored value.
    #[test]
    fn init_runs_once() {
        static B: OnceBox<[u32; 4]> = OnceBox::new();
        let a = B.get_or_init(|| Box::new([1, 2, 3, 4]));
        // Already initialized: the second closure must not replace it.
        let b = B.get_or_init(|| Box::new([9, 9, 9, 9]));
        assert_eq!(a, &[1, 2, 3, 4]);
        assert!(core::ptr::eq(a, b));
    }

    /// Exercises the compare_exchange race path; under miri's thread
    /// interleaving this checks the atomic ordering is sound (no torn read, no
    /// leak-then-use).
    #[cfg(feature = "std")]
    #[test]
    fn concurrent_init_is_sound() {
        use alloc::sync::Arc;
        use alloc::vec::Vec;
        let cell = Arc::new(OnceBox::<u64>::new());
        let handles: Vec<_> = (0..4u64)
            .map(|i| {
                let c = cell.clone();
                std::thread::spawn(move || *c.get_or_init(|| Box::new(0xABCD_0000 + i)))
            })
            .collect();
        let seen: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        // Every thread observes the same winning value.
        assert!(seen.iter().all(|&v| v == seen[0]));
        assert_eq!(seen[0] & 0xFFFF_0000, 0xABCD_0000);
    }
}
