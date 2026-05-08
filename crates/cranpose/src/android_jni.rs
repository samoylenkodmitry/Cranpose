//! Shared Android JNI helpers.

use jni::{objects::JObject, JNIEnv, JavaVM};

pub(crate) fn with_android_activity_env<T, F>(
    app: &android_activity::AndroidApp,
    f: F,
) -> Result<T, String>
where
    F: for<'local> FnOnce(&mut JNIEnv<'local>, JObject<'local>) -> Result<T, String>,
{
    // SAFETY: android-activity owns a valid JavaVM pointer for the lifetime of AndroidApp.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }
        .map_err(|error| format!("failed to access Android Java VM: {error}"))?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| format!("failed to attach Android JNI thread: {error}"))?;

    // SAFETY: activity_as_ptr returns a JNI global Activity reference owned by android-activity.
    // JObject does not delete this reference on drop; it is used only for this JNI call chain.
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    f(&mut env, activity)
}

pub(crate) fn clear_pending_android_jni_exception(env: &mut JNIEnv<'_>) {
    if matches!(env.exception_check(), Ok(true)) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
}
