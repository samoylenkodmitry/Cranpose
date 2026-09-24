use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use cranpose_ui::{Point, Size};
use jni::{
    Env, jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::jlong,
};
use ndk::native_window::NativeWindow;

use crate::{
    android_host_window,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    app_launcher::AndroidOverlayWindowOptions,
};

const OVERLAY_CLASS: &str = "dev/cranpose/android/CranposeOverlayWindow";
const RESULT_OK: i32 = 0;
const RESULT_ALREADY_VISIBLE: i32 = -4;
const RESULT_NOT_VISIBLE: i32 = -6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AndroidOverlayWindowBounds {
    width_px: i32,
    height_px: i32,
    x_px: i32,
    y_px: i32,
}

#[derive(Debug)]
pub(crate) enum AndroidOverlayWindowEvent {
    CreateFailed(String),
    SurfaceChanged {
        native_window: NativeWindow,
        width: u32,
        height: u32,
    },
    SurfaceDestroyed,
    Pointer {
        action: AndroidOverlayPointerAction,
        x: f32,
        y: f32,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AndroidOverlayPointerAction {
    Down,
    Up,
    Move,
    Cancel,
}

#[derive(Default)]
pub(crate) struct AndroidOverlayEventQueue {
    events: Mutex<VecDeque<AndroidOverlayWindowEvent>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AndroidOverlayEventQueueHandle(jlong);

impl AndroidOverlayEventQueueHandle {
    pub(crate) fn raw(self) -> jlong {
        self.0
    }
}

impl AndroidOverlayEventQueue {
    pub(crate) fn push(&self, event: AndroidOverlayWindowEvent) {
        self.lock_events().push_back(event);
    }

    fn drain(&self) -> Vec<AndroidOverlayWindowEvent> {
        self.lock_events().drain(..).collect()
    }

    fn lock_events(&self) -> MutexGuard<'_, VecDeque<AndroidOverlayWindowEvent>> {
        self.events.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub(crate) fn retain_android_overlay_event_queue_handle(
    queue: &Arc<AndroidOverlayEventQueue>,
) -> AndroidOverlayEventQueueHandle {
    let pointer = Arc::into_raw(Arc::clone(queue));
    AndroidOverlayEventQueueHandle(pointer as usize as jlong)
}

pub(crate) fn show_android_overlay_window(
    app: &android_activity::AndroidApp,
    options: AndroidOverlayWindowOptions,
    density: f32,
    event_queue: &Arc<AndroidOverlayEventQueue>,
) -> Result<(), String> {
    let bounds = overlay_options_to_physical_bounds(options, density)?;
    let event_queue_handle = retain_android_overlay_event_queue_handle(event_queue);

    let result = with_android_activity_env(app, |env, activity| {
        let class = find_android_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                jni_str!("show"),
                jni_sig!("(Landroid/app/Activity;IIIIZJ)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(bounds.width_px),
                    JValue::Int(bounds.height_px),
                    JValue::Int(bounds.x_px),
                    JValue::Int(bounds.y_px),
                    JValue::Bool(options.focusable),
                    JValue::Long(event_queue_handle.raw()),
                ],
            )
            .and_then(jni::JValueOwned::i)
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to show Android overlay window: {error}")
            })?;

        Ok(result)
    });

    match result {
        Ok(RESULT_OK) => Ok(()),
        Ok(RESULT_ALREADY_VISIBLE) => {
            crate::android_jni::release_android_overlay_event_queue_handle(event_queue_handle);
            Ok(())
        }
        Ok(code) => {
            crate::android_jni::release_android_overlay_event_queue_handle(event_queue_handle);
            Err(format_android_overlay_result(code))
        }
        Err(error) => {
            crate::android_jni::release_android_overlay_event_queue_handle(event_queue_handle);
            Err(error)
        }
    }
}

pub(crate) fn update_android_overlay_window_bounds(
    app: &android_activity::AndroidApp,
    position: Point,
    size: Size,
    density: f32,
) -> Result<(), String> {
    let bounds = overlay_bounds_to_physical(position, size, density)?;

    with_android_activity_env(app, |env, activity| {
        let class = find_android_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                jni_str!("updateBounds"),
                jni_sig!("(Landroid/app/Activity;IIII)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(bounds.width_px),
                    JValue::Int(bounds.height_px),
                    JValue::Int(bounds.x_px),
                    JValue::Int(bounds.y_px),
                ],
            )
            .and_then(jni::JValueOwned::i)
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to update Android overlay window bounds: {error}")
            })?;

        match result {
            RESULT_OK => Ok(()),
            code => Err(format_android_overlay_result(code)),
        }
    })
}

pub(crate) fn hide_android_overlay_window(app: &android_activity::AndroidApp) {
    let _ = with_android_activity_env(app, |env, activity| {
        let class = find_android_overlay_class(env, &activity)?;
        env.call_static_method(
            class,
            jni_str!("hide"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(&activity)],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to hide Android overlay window: {error}")
        })?;
        Ok(())
    });
}

pub(crate) fn drain_android_overlay_window_events(
    queue: &AndroidOverlayEventQueue,
) -> Vec<AndroidOverlayWindowEvent> {
    queue.drain()
}

pub(crate) fn find_android_overlay_class<'local>(
    env: &mut Env<'local>,
    activity: &JObject<'local>,
) -> Result<JClass<'local>, String> {
    crate::android_jni::load_cranpose_java_class(env, activity, OVERLAY_CLASS)
}

fn overlay_options_to_physical_bounds(
    options: AndroidOverlayWindowOptions,
    density: f32,
) -> Result<AndroidOverlayWindowBounds, String> {
    if !options.is_valid() {
        return Err("Android overlay window dimensions must be greater than zero".to_string());
    }
    overlay_bounds_to_physical(
        Point::new(options.x as f32, options.y as f32),
        Size::new(options.width as f32, options.height as f32),
        density,
    )
}

fn overlay_bounds_to_physical(
    position: Point,
    size: Size,
    density: f32,
) -> Result<AndroidOverlayWindowBounds, String> {
    let size =
        android_host_window::validate_logical_size(size).map_err(|error| error.to_string())?;
    Ok(AndroidOverlayWindowBounds {
        width_px: logical_dimension_to_physical_px(size.width, density)?,
        height_px: logical_dimension_to_physical_px(size.height, density)?,
        x_px: logical_to_physical_px(position.x, density)?,
        y_px: logical_to_physical_px(position.y, density)?,
    })
}

fn logical_dimension_to_physical_px(value: f32, density: f32) -> Result<i32, String> {
    Ok(logical_to_physical_px(value, density)?.max(1))
}

fn logical_to_physical_px(value: f32, density: f32) -> Result<i32, String> {
    if !value.is_finite() {
        return Err("Android overlay dimensions and coordinates must be finite".to_string());
    }
    if !density.is_finite() || density <= 0.0 {
        return Err("Android display density must be positive and finite".to_string());
    }
    let rounded = (value * density).round();
    if rounded < i32::MIN as f32 || rounded > i32::MAX as f32 {
        return Err("Android overlay physical coordinate is outside i32 range".to_string());
    }
    Ok(rounded as i32)
}

fn format_android_overlay_result(code: i32) -> String {
    match code {
        -1 => "Android overlay windows require Android 8.0/API 26 or newer".to_string(),
        -2 => {
            "Android overlay window requires android.permission.SYSTEM_ALERT_WINDOW in the manifest"
                .to_string()
        }
        -3 => "Android overlay window permission is not granted by the user".to_string(),
        -5 => "Android overlay window creation failed on the Java UI thread".to_string(),
        RESULT_NOT_VISIBLE => "Android overlay window is not visible".to_string(),
        _ => format!("Android overlay window helper returned error code {code}"),
    }
}

#[cfg(test)]
#[path = "tests/android_overlay_window_tests.rs"]
mod tests;
