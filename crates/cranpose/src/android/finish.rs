use jni::{jni_sig, jni_str};

use super::jni::{clear_pending_android_jni_exception, with_android_activity_env};

/// Finishes the activity of `app`, as Android's `Activity.finish()`. Returns whether the call
/// landed: `false` means the activity is still up, and the app may try again.
///
/// The event loop of Cranpose calls it when the app calls
/// [`request_exit`](cranpose_services::request_exit). An app that runs its own loop, such as an
/// immersive headset app, calls it when it has stopped what it runs and
/// [`exit_requested`](cranpose_services::exit_requested) is true. It then takes the request with
/// [`take_exit_request`](cranpose_services::take_exit_request).
pub fn finish_activity(app: &android_activity::AndroidApp) -> bool {
    let finished = with_android_activity_env(app, |env, activity| {
        env.call_method(&activity, jni_str!("finish"), jni_sig!("()V"), &[])
            .map(|_| ())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("Activity.finish() failed: {error}")
            })
    });
    match finished {
        Ok(()) => true,
        Err(error) => {
            log::error!("could not finish the activity: {error}");
            false
        }
    }
}
