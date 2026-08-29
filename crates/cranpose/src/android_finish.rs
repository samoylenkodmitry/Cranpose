use jni::{jni_sig, jni_str};

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};

pub(crate) fn finish_activity(app: &android_activity::AndroidApp) -> bool {
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
