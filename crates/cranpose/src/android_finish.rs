//! Closing the activity from inside the app.
//!
//! One NDK symbol, `ANativeActivity_finish`, and the safe call through it. It
//! is the whole of the platform API behind [`crate::request_exit`] on Android:
//! `android-activity` hands out the `ANativeActivity*` but wraps nothing that
//! finishes it, and the C function is documented as safe to call from any
//! thread — it posts a message to the activity's own looper rather than tearing
//! anything down where it is called.
//!
//! `finish()` does not end the process or stop this frame. The activity is
//! destroyed asynchronously and arrives back at the run loop as
//! `MainEvent::Destroy`, which is the same path the system's own back takes, so
//! an app-requested exit and a user-requested one leave through the same door.

#![allow(unsafe_code)]

use std::ffi::c_void;

extern "C" {
    fn ANativeActivity_finish(activity: *mut c_void);
}

/// Asks the platform to finish the activity.
///
/// `activity` must be the `ANativeActivity*` from the `AndroidApp` the run loop
/// was handed; a null pointer is ignored rather than passed on.
pub(crate) fn finish_activity(activity: *mut c_void) {
    if activity.is_null() {
        log::warn!("cannot finish the activity: no ANativeActivity pointer");
        return;
    }
    // SAFETY: the pointer comes from `AndroidApp::activity_as_ptr` and the
    // activity outlives the run loop that owns that handle. The call posts to
    // the activity's looper and returns; it dereferences nothing here.
    unsafe { ANativeActivity_finish(activity) };
}
