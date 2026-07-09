//! Background execution: ask the OS to keep running briefly after the app is
//! backgrounded, so in-flight work (e.g. draining a recognition queue) can
//! finish instead of being suspended immediately.
//!
//! The platform backend installs an implementation via
//! [`set_platform_background_activity`] (iOS `beginBackgroundTask`, Android a
//! foreground service). No default: [`background_activity`] returns `None` where
//! unsupported, and the app simply runs only while foregrounded.

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

/// Convenience: mark background work active/inactive (no-op where unsupported).
pub fn set_background_active(active: bool) {
    if let Some(activity) = background_activity() {
        activity.set_active(active);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn registration_round_trips() {
        clear_platform_background_activity();
        set_background_active(true); // no-op, no panic
        struct Rec(AtomicBool);
        impl BackgroundActivity for Rec {
            fn set_active(&self, active: bool) {
                self.0.store(active, Ordering::SeqCst);
            }
        }
        let rec = Arc::new(Rec(AtomicBool::new(false)));
        set_platform_background_activity(rec.clone());
        set_background_active(true);
        assert!(rec.0.load(Ordering::SeqCst));
        clear_platform_background_activity();
    }
}
