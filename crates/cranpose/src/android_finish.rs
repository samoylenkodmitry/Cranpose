//! Closing the activity from inside the app: the Android half of
//! [`crate::request_exit`].
//!
//! It is a JNI call to `Activity.finish()`, and it has to be.
//! `ANativeActivity_finish` looks like the obvious answer and is a trap:
//! `AndroidApp::activity_as_ptr` returns a **jobject** global reference to the
//! Activity, not an `ANativeActivity*` — android-activity's own source says so,
//! and `android_jni` in this crate has always treated it as a jobject. Passing
//! it to the NDK function type-confuses the two. Measured on a Pixel Watch 3
//! before this was understood: `app died, no saved state` and `exited due to
//! signal 9 (Killed)`, followed by a second of black while the system cleaned
//! up. That reads exactly like a working-but-slow exit, which is what makes it
//! worth a module of its own with the reason written down.
//!
//! `finish()` does not end the process or stop this frame. The activity is
//! destroyed asynchronously and arrives back at the run loop as
//! `MainEvent::Destroy`, which is the same path the system's own back takes, so
//! an app-requested exit and a user-requested one leave through the same door.

use crate::android_jni::with_android_activity_env;
use jni::{jni_sig, jni_str};

/// Asks the platform to finish the activity.
pub(crate) fn finish_activity(app: &android_activity::AndroidApp) {
    let finished = with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, jni_str!("finish"), jni_sig!("()V"), &[])
            .map(|_| ())
            .map_err(|error| format!("Activity.finish() failed: {error}"))
    });
    if let Err(error) = finished {
        log::error!("could not finish the activity: {error}");
    }
}
