//! Background execution: ask the OS to keep running briefly after the app is
//! backgrounded, so in-flight work (e.g. draining a recognition queue) can
//! finish instead of being suspended immediately.
//!
//! The platform backend installs an implementation via
//! [`set_platform_background_activity`] (iOS `beginBackgroundTask`, Android a
//! foreground service). No default: [`background_activity`] returns `None` where
//! unsupported, and the app simply runs only while foregrounded.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Marks periods of important work so the platform grants background running
/// time. Implementations are `Send + Sync` (the app toggles activity from its
/// worker threads).
pub trait BackgroundActivity: Send + Sync {
    /// `true` when important work starts, `false` when it finishes. Calls are
    /// balanced by the app but implementations must tolerate repeats.
    fn set_active(&self, active: bool);
}

pub type BackgroundActivityRef = Arc<dyn BackgroundActivity>;

fn slot() -> &'static Mutex<Option<BackgroundActivityRef>> {
    static SLOT: OnceLock<Mutex<Option<BackgroundActivityRef>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the platform background-activity handler, replacing any previous one.
pub fn set_platform_background_activity(activity: BackgroundActivityRef) {
    if background_active() {
        activity.set_active(true);
    }
    if let Ok(mut s) = slot().lock() {
        *s = Some(activity);
    }
}

/// Removes any registered handler (tests/teardown).
pub fn clear_platform_background_activity() {
    if let Ok(mut s) = slot().lock() {
        *s = None;
    }
}

/// The registered background-activity handler, or `None` where unsupported.
pub fn background_activity() -> Option<BackgroundActivityRef> {
    slot().lock().ok().and_then(|s| s.clone())
}

static ACTIVE_LEASES: AtomicUsize = AtomicUsize::new(0);

/// A claim that important work needs the host's background execution allowance.
/// The allowance remains active until every outstanding lease is dropped.
pub struct BackgroundWorkLease {
    active: bool,
}

impl Drop for BackgroundWorkLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        if ACTIVE_LEASES.fetch_sub(1, Ordering::AcqRel) == 1 {
            if let Some(activity) = background_activity() {
                activity.set_active(false);
            }
        }
    }
}

/// Acquires background execution for one independent operation.
pub fn acquire_background_work() -> BackgroundWorkLease {
    if ACTIVE_LEASES.fetch_add(1, Ordering::AcqRel) == 0 {
        if let Some(activity) = background_activity() {
            activity.set_active(true);
        }
    }
    BackgroundWorkLease { active: true }
}

/// Whether at least one background-work lease is active.
///
/// A platform backend reads this to decide whether the runtime keeps turning
/// while the app is off screen. Both mobile backends drive composition from the
/// frame path, and both stop that path when the surface is gone: Android drops
/// its GPU resources on `TerminateWindow`, iOS stops receiving redraws. Anything
/// posted to the UI dispatcher then waits until the user comes back, which is
/// wrong for an app that told the OS it has work to finish.
pub fn background_active() -> bool {
    ACTIVE_LEASES.load(Ordering::Acquire) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn registration_round_trips() {
        // The lease count is one number for the process, and the media service
        // takes a lease of its own while it plays. Tests that read it take
        // turns, or each sees the other's leases as its own.
        let _services = crate::registry::test_service_guard();
        clear_platform_background_activity();
        struct Rec(AtomicBool);
        impl BackgroundActivity for Rec {
            fn set_active(&self, active: bool) {
                self.0.store(active, Ordering::SeqCst);
            }
        }
        let rec = Arc::new(Rec(AtomicBool::new(false)));
        set_platform_background_activity(rec.clone());
        let first = acquire_background_work();
        assert!(rec.0.load(Ordering::SeqCst));
        assert!(background_active());
        let second = acquire_background_work();
        drop(first);
        assert!(background_active());
        assert!(rec.0.load(Ordering::SeqCst));
        drop(second);
        assert!(!background_active());
        assert!(!rec.0.load(Ordering::SeqCst));
        clear_platform_background_activity();
    }
}
