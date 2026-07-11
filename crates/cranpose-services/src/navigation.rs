//! System "back" navigation requests.
//!
//! A platform back affordance — Android's back key / gesture, iOS's left-edge
//! swipe — feeds [`push_back_request`]; the app drains it with
//! [`take_back_requests`] and pops its own navigation. This gives one API
//! across platforms for what is otherwise a per-OS gesture.
//!
//! Whether the platform *routes* its back control here is governed by
//! [`set_back_interception`], the analogue of Compose's `BackHandler(enabled)`:
//!
//! - **Android**: while interception is enabled the back key/gesture is
//!   consumed and lands in [`push_back_request`]; while disabled it stays with
//!   the system, so the default behavior (leaving the activity) keeps working.
//!   Apps enable it exactly while they have somewhere to navigate back to.
//! - **iOS**: the left-edge swipe is a framework-drawn gesture with no system
//!   fallback, so it always pushes a request regardless of interception.
//! - **Desktop/web**: no OS back control; apps may map keys themselves and
//!   call [`push_back_request`] directly.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

static BACK_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static BACK_INTERCEPTION: AtomicBool = AtomicBool::new(false);

/// Record a system back request (called by the platform backend's gesture /
/// button handler).
pub fn push_back_request() {
    BACK_REQUESTS.fetch_add(1, Ordering::SeqCst);
}

/// Take (and clear) the number of pending back requests. Polled by the app; a
/// burst collapses into a count the app can coalesce.
pub fn take_back_requests() -> usize {
    BACK_REQUESTS.swap(0, Ordering::SeqCst)
}

/// Declare whether the app currently wants the platform's back control routed
/// to [`push_back_request`] instead of the platform default. Set it `true`
/// while there is in-app navigation to pop and `false` when leaving the app is
/// the right response (mirrors Compose's `BackHandler(enabled)`).
pub fn set_back_interception(enabled: bool) {
    BACK_INTERCEPTION.store(enabled, Ordering::SeqCst);
}

/// Whether the app asked to intercept the platform back control. Read by the
/// platform input path.
pub fn back_interception_enabled() -> bool {
    BACK_INTERCEPTION.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_accumulate_and_drain() {
        let _ = take_back_requests(); // clear any residue
        push_back_request();
        push_back_request();
        assert_eq!(take_back_requests(), 2);
        assert_eq!(take_back_requests(), 0);
    }

    #[test]
    fn interception_defaults_off_and_toggles() {
        set_back_interception(false);
        assert!(!back_interception_enabled());
        set_back_interception(true);
        assert!(back_interception_enabled());
        set_back_interception(false);
    }
}
