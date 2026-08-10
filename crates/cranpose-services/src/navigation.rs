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
use std::sync::OnceLock;

type BackListener = Box<dyn Fn() + Send + Sync + 'static>;

static BACK_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static BACK_INTERCEPTION: AtomicBool = AtomicBool::new(false);
static BACK_LISTENER: OnceLock<BackListener> = OnceLock::new();

/// Record a system back request (called by the platform backend's gesture /
/// button handler).
pub fn push_back_request() {
    BACK_REQUESTS.fetch_add(1, Ordering::SeqCst);
    if let Some(listener) = BACK_LISTENER.get() {
        listener();
    }
}

/// Registers a callback run whenever a back request arrives, so an app can be
/// told rather than having to ask.
///
/// [`take_back_requests`] alone is a polling API, which quietly assumes the app
/// is already running a frame loop to poll from. An app that has gone idle —
/// the correct thing to do on a screen where nothing moves — has no such loop,
/// and a back gesture would sit in the counter until something unrelated woke
/// it. The listener closes that gap: it is the nudge, the counter is still the
/// source of truth, and the app drains it as before.
///
/// Called from whatever thread the platform reports back on, which is not
/// necessarily the UI thread, so the callback must be `Send + Sync`. It should
/// do as little as possible — waking a parked task is the intended use.
///
/// Only the first registration takes effect; a second is ignored, since two
/// owners of the app's back handling would each see a request the other also
/// consumed.
pub fn set_back_request_listener(listener: impl Fn() + Send + Sync + 'static) {
    let _ = BACK_LISTENER.set(Box::new(listener));
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
    use std::sync::Arc;

    #[test]
    fn requests_accumulate_and_drain() {
        let _ = take_back_requests(); // clear any residue
        push_back_request();
        push_back_request();
        assert_eq!(take_back_requests(), 2);
        assert_eq!(take_back_requests(), 0);
    }

    #[test]
    fn a_registered_listener_hears_every_request() {
        let heard = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&heard);
        set_back_request_listener(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let before = heard.load(Ordering::SeqCst);
        push_back_request();
        push_back_request();
        assert_eq!(heard.load(Ordering::SeqCst), before + 2);
        let _ = take_back_requests();
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
