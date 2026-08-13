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
//!
//! [`request_exit`] is the other direction: the app, rather than the platform,
//! deciding that it is time to leave.

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

type BackListener = Box<dyn Fn() + Send + Sync + 'static>;

static BACK_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static BACK_INTERCEPTION: AtomicBool = AtomicBool::new(false);
static BACK_LISTENER: OnceLock<BackListener> = OnceLock::new();
/// A flag rather than a count: closing twice is closing once.
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);

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

/// Ask the platform to close the app.
///
/// The counterpart to [`set_back_interception`]`(false)`: interception says
/// "let the platform's own back control take me out of here", and this says
/// the same thing when the app is the one that decided. An app needs it
/// whenever it owns the affordance that means "leave":
///
/// - a screen-level dismiss gesture the app draws itself. On Android a
///   `NativeActivity` consumes every pointer event on the display, so the
///   platform's window-level swipe never fires and the app's own gesture is
///   the only one there is — completing it has to close the app, and there is
///   no back key on a watch to fall back on.
/// - a Quit item in the app's own menu.
///
/// It is a request, not a teardown: the platform decides when the frame loop
/// stops, so it is safe to call from the middle of one — including from a
/// gesture's settle animation, which is where the decision usually lands.
///
/// The backend drains it on its next turn of the loop, which is the following
/// frame for the usual caller. An app that calls this from another thread while
/// the loop is parked — nothing animating, no input — should wake it the same
/// way it would for a back request; registering
/// [`set_back_request_listener`] is enough, because this nudges that listener
/// too.
///
/// Platform behaviour, and where it does nothing:
///
/// - **Android**: finishes the activity, the same outcome as Compose's
///   `backDispatcher.onBackPressed()` on a screen with no `BackHandler`.
/// - **Desktop**: exits the event loop, closing the window.
/// - **iOS**: nothing. Apple's guidelines forbid an app terminating itself and
///   there is no supported API for it; the request is dropped rather than
///   faked, so an app can call this unconditionally.
/// - **Web**: nothing. A page cannot close a tab it did not open.
pub fn request_exit() {
    EXIT_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(listener) = BACK_LISTENER.get() {
        // The same nudge a back request gets, and for the same reason: an app
        // that has gone idle has no frame loop to notice the flag, and the
        // platform drains it from that loop.
        listener();
    }
}

/// Whether an exit request is outstanding, without consuming it.
///
/// For a backend whose way of closing can fail. Consuming the flag and then
/// discovering the platform call did not land loses the app's only record that
/// it wanted to close: the app stays open, the gesture the user made did
/// nothing, and nothing will ever ask again. Such a backend tests with this and
/// calls [`take_exit_request`] once the request has actually been honoured.
///
/// This is also the cheap read for a loop that runs it every turn — see
/// [`take_exit_request`].
pub fn exit_requested() -> bool {
    EXIT_REQUESTED.load(Ordering::SeqCst)
}

/// Take (and clear) a pending exit request. Drained by the platform backend.
///
/// Read before written: this runs on every turn of the platform's loop, and a
/// bare `swap` would dirty the cache line each time even with nothing to take.
/// The load is the common case by a very long way — an app asks to close once,
/// ever.
pub fn take_exit_request() -> bool {
    exit_requested() && EXIT_REQUESTED.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Every global in this module is process-wide and the runner is threaded,
    /// so the tests take turns. They share one lock rather than one each: the
    /// back-request counter, the exit flag and the listener are entangled --
    /// `request_exit` nudges the listener, so a test asserting on the listener
    /// count is moved by a test that only meant to touch the exit flag. One
    /// failure in twenty, which is the worst rate for anyone to debug.
    fn navigation_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn requests_accumulate_and_drain() {
        let _guard = navigation_lock();
        let _ = take_back_requests(); // clear any residue
        push_back_request();
        push_back_request();
        assert_eq!(take_back_requests(), 2);
        assert_eq!(take_back_requests(), 0);
    }

    /// The listener is a `OnceLock`, so exactly one test in this process can
    /// own it -- a second registration is silently ignored, and a test that
    /// registered its own would sit there hearing nothing. Everything that has
    /// to observe the nudge is therefore checked here.
    #[test]
    fn a_registered_listener_hears_every_request() {
        let _guard = navigation_lock();
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

        // An exit request nudges it too. The flag is drained from the frame
        // loop, and an app with nothing moving has parked that loop; without
        // the nudge the request would sit there until something unrelated
        // woke the app.
        let before = heard.load(Ordering::SeqCst);
        request_exit();
        assert_eq!(
            heard.load(Ordering::SeqCst),
            before + 1,
            "an exit request has to wake an idle app the way a back request does"
        );
        let _ = take_exit_request();
    }

    #[test]
    fn an_exit_request_is_taken_once() {
        let _guard = navigation_lock();
        let _ = take_exit_request(); // clear any residue
        assert!(!take_exit_request());
        request_exit();
        // Twice, because closing twice is closing once and a settle animation
        // can easily ask on two consecutive frames.
        request_exit();
        assert!(take_exit_request());
        assert!(
            !take_exit_request(),
            "a drained request came back; the platform would close twice"
        );
    }

    #[test]
    fn a_backend_can_look_at_the_request_without_consuming_it() {
        let _guard = navigation_lock();
        // The Android backend's way of closing is a JNI call that can fail.
        // Consuming the flag first and then discovering the call did not land
        // loses the app's only record that it asked to close: it stays open,
        // the gesture the user made did nothing, and nothing asks again.
        let _ = take_exit_request();
        assert!(!exit_requested());

        request_exit();
        assert!(exit_requested());
        assert!(
            exit_requested(),
            "looking at the request consumed it, which is the bug"
        );

        assert!(take_exit_request());
        assert!(!exit_requested());
    }

    #[test]
    fn interception_defaults_off_and_toggles() {
        let _guard = navigation_lock();
        set_back_interception(false);
        assert!(!back_interception_enabled());
        set_back_interception(true);
        assert!(back_interception_enabled());
        set_back_interception(false);
    }
}
