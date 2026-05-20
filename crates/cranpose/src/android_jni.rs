//! Shared Android JNI helpers.

use jni::{objects::JObject, Env, JavaVM};

pub(crate) fn with_android_activity_env<T, F>(
    app: &android_activity::AndroidApp,
    f: F,
) -> Result<T, String>
where
    F: for<'local> FnOnce(&mut Env<'local>, JObject<'local>) -> Result<T, String>,
{
    // SAFETY: android-activity owns a valid JavaVM pointer for the lifetime of AndroidApp.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<Result<T, String>> {
        // SAFETY: activity_as_ptr returns a JNI global Activity reference owned by android-activity.
        // JObject does not delete this reference on drop; it is used only for this JNI call chain.
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        Ok(f(env, activity))
    })
    .map_err(|error| format!("failed to attach Android JNI thread: {error}"))?
}

pub(crate) fn clear_pending_android_jni_exception(env: &mut Env<'_>) {
    if env.exception_check() {
        env.exception_describe();
        env.exception_clear();
    }
}
