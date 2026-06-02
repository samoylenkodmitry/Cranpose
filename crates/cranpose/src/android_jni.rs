//! Shared Android JNI helpers.
#![allow(unsafe_code)]

use crate::android_overlay_window::{
    AndroidOverlayEventQueue, AndroidOverlayEventQueueHandle, AndroidOverlayPointerAction,
    AndroidOverlayWindowEvent,
};
use jni::{
    objects::{JClass, JObject, JString},
    sys::{jfloat, jint, jlong},
    Env, EnvUnowned, JavaVM, Outcome,
};
use ndk::native_window::NativeWindow;
use std::sync::Arc;

pub(crate) fn with_android_activity_env<T, F>(
    app: &android_activity::AndroidApp,
    f: F,
) -> Result<T, String>
where
    F: for<'local> FnOnce(&mut Env<'local>, JObject<'local>) -> Result<T, String>,
{
    let vm = JavaVM::singleton()
        .map_err(|error| format!("Android JavaVM is not available on android_main: {error}"))?;
    vm.with_local_frame(16, |env| -> jni::errors::Result<Result<T, String>> {
        let raw_activity_global = app.activity_as_ptr() as jni::sys::jobject;
        // SAFETY: android-activity owns this unowned global Activity reference for the
        // AndroidApp lifetime. The cast borrows it without taking deletion ownership;
        // new_local_ref creates the scoped local reference used by this JNI call chain.
        let activity_global = unsafe { env.as_cast_raw::<JObject>(&raw_activity_global)? };
        let activity = env.new_local_ref(activity_global.as_ref())?;
        Ok(f(env, activity))
    })
    .map_err(|error| format!("failed to access Android JNI environment: {error}"))?
}

pub(crate) fn clear_pending_android_jni_exception(env: &mut Env<'_>) {
    if env.exception_check() {
        env.exception_describe();
        env.exception_clear();
    }
}

pub(crate) fn native_window_from_surface(
    env: &mut Env<'_>,
    surface: JObject<'_>,
) -> Result<NativeWindow, String> {
    // SAFETY: The Java overlay helper calls this with the current JNI
    // environment and a live android.view.Surface from SurfaceHolder.
    unsafe { NativeWindow::from_surface(env.get_raw().cast(), surface.as_raw()) }.ok_or_else(|| {
        clear_pending_android_jni_exception(env);
        "Android overlay Surface did not provide an ANativeWindow".to_string()
    })
}

pub(crate) fn release_android_overlay_event_queue_handle(handle: AndroidOverlayEventQueueHandle) {
    release_android_overlay_event_queue_handle_raw(handle.raw());
}

fn release_android_overlay_event_queue_handle_raw(handle: jlong) {
    if handle == 0 {
        return;
    }

    // SAFETY: AndroidOverlayEventQueueHandle values are created by
    // Arc::into_raw in android_overlay_window and released exactly once by the
    // Java overlay helper or by Rust when show() did not transfer ownership.
    unsafe {
        drop(Arc::from_raw(
            handle as usize as *const AndroidOverlayEventQueue,
        ));
    }
}

fn push_overlay_event_for_handle(handle: jlong, event: AndroidOverlayWindowEvent) {
    if handle == 0 {
        log::warn!(
            "dropped Android overlay event because the Java callback carried no queue handle"
        );
        return;
    }

    // SAFETY: The Java overlay helper stores a handle retained from an
    // Arc<AndroidOverlayEventQueue> and releases it only after the overlay is
    // disposed on the Android UI thread. Callbacks are ignored after disposal.
    let Some(queue) = (unsafe { (handle as usize as *const AndroidOverlayEventQueue).as_ref() })
    else {
        log::warn!("dropped Android overlay event because the queue handle was invalid");
        return;
    };
    queue.push(event);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlayCreateFailed<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    queue_handle: jlong,
    message: JString<'local>,
) {
    let message = match env
        .with_env(|env| -> jni::errors::Result<String> { message.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(message) => message,
        Outcome::Err(_) | Outcome::Panic(_) => "Android overlay window creation failed".to_string(),
    };
    push_overlay_event_for_handle(
        queue_handle,
        AndroidOverlayWindowEvent::CreateFailed(message),
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlaySurfaceChanged<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    queue_handle: jlong,
    surface: JObject<'local>,
    width: jint,
    height: jint,
) {
    match env
        .with_env(|env| -> jni::errors::Result<Result<NativeWindow, String>> {
            Ok(native_window_from_surface(env, surface))
        })
        .into_outcome()
    {
        Outcome::Ok(Ok(native_window)) if width > 0 && height > 0 => {
            push_overlay_event_for_handle(
                queue_handle,
                AndroidOverlayWindowEvent::SurfaceChanged {
                    native_window,
                    width: width as u32,
                    height: height as u32,
                },
            );
        }
        Outcome::Ok(Ok(_)) => {}
        Outcome::Ok(Err(message)) => {
            push_overlay_event_for_handle(
                queue_handle,
                AndroidOverlayWindowEvent::CreateFailed(message),
            );
        }
        Outcome::Err(error) => {
            push_overlay_event_for_handle(
                queue_handle,
                AndroidOverlayWindowEvent::CreateFailed(format!(
                    "failed to access Android overlay Surface: {error}"
                )),
            );
        }
        Outcome::Panic(_) => {
            push_overlay_event_for_handle(
                queue_handle,
                AndroidOverlayWindowEvent::CreateFailed(
                    "failed to access Android overlay Surface".to_string(),
                ),
            );
        }
    }
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlaySurfaceDestroyed(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    push_overlay_event_for_handle(queue_handle, AndroidOverlayWindowEvent::SurfaceDestroyed);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlayPointer(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    action: jint,
    x: jfloat,
    y: jfloat,
) {
    let action = match action {
        0 | 5 => AndroidOverlayPointerAction::Down,
        1 | 6 => AndroidOverlayPointerAction::Up,
        2 => AndroidOverlayPointerAction::Move,
        3 => AndroidOverlayPointerAction::Cancel,
        _ => return,
    };
    push_overlay_event_for_handle(
        queue_handle,
        AndroidOverlayWindowEvent::Pointer { action, x, y },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlayReleaseQueue(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    release_android_overlay_event_queue_handle_raw(queue_handle);
}
