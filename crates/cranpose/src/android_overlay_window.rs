//! Android floating overlay window bridge.

use crate::{
    android_host_window,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    launcher::AndroidOverlayWindowOptions,
};
use cranpose_ui::{Point, Size};
use jni::{
    objects::{GlobalRef, JClass, JObject, JString, JValue},
    sys::{jfloat, jint},
    JNIEnv,
};
use ndk::native_window::NativeWindow;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
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

pub(crate) fn show_android_overlay_window(
    app: &android_activity::AndroidApp,
    options: AndroidOverlayWindowOptions,
    density: f32,
) -> Result<(), String> {
    let bounds = overlay_options_to_physical_bounds(options, density)?;

    with_android_activity_env(app, |env, activity| {
        let class = find_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                "show",
                "(Landroid/app/Activity;IIIIZ)I",
                &[
                    JValue::Object(&activity),
                    JValue::Int(bounds.width_px),
                    JValue::Int(bounds.height_px),
                    JValue::Int(bounds.x_px),
                    JValue::Int(bounds.y_px),
                    JValue::Bool(u8::from(options.focusable)),
                ],
            )
            .and_then(|value| value.i())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to show Android overlay window: {error}")
            })?;

        match result {
            RESULT_OK | RESULT_ALREADY_VISIBLE => Ok(()),
            code => Err(format_android_overlay_result(code)),
        }
    })
}

pub(crate) fn update_android_overlay_window_bounds(
    app: &android_activity::AndroidApp,
    position: Point,
    size: Size,
    density: f32,
) -> Result<(), String> {
    let bounds = overlay_bounds_to_physical(position, size, density)?;

    with_android_activity_env(app, |env, activity| {
        let class = find_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                "updateBounds",
                "(Landroid/app/Activity;IIII)I",
                &[
                    JValue::Object(&activity),
                    JValue::Int(bounds.width_px),
                    JValue::Int(bounds.height_px),
                    JValue::Int(bounds.x_px),
                    JValue::Int(bounds.y_px),
                ],
            )
            .and_then(|value| value.i())
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
        let class = find_overlay_class(env, &activity)?;
        env.call_static_method(
            class,
            "hide",
            "(Landroid/app/Activity;)V",
            &[JValue::Object(&activity)],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to hide Android overlay window: {error}")
        })?;
        Ok(())
    });
}

pub(crate) fn drain_android_overlay_window_events() -> Vec<AndroidOverlayWindowEvent> {
    let mut events = overlay_events()
        .lock()
        .expect("overlay event queue poisoned");
    events.drain(..).collect()
}

fn find_overlay_class<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
) -> Result<&'static GlobalRef, String> {
    static OVERLAY_CLASS_REF: OnceLock<GlobalRef> = OnceLock::new();

    if let Some(class) = OVERLAY_CLASS_REF.get() {
        return Ok(class);
    }

    let class = load_overlay_class(env, activity)?;
    let global_class = env.new_global_ref(class).map_err(|error| {
        clear_pending_android_jni_exception(env);
        format!("failed to cache Android overlay helper class: {error}")
    })?;

    let _ = OVERLAY_CLASS_REF.set(global_class);
    OVERLAY_CLASS_REF.get().ok_or_else(|| {
        "failed to cache Android overlay helper class in process-global storage".to_string()
    })
}

fn load_overlay_class<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject<'local>,
) -> Result<JClass<'local>, String> {
    let class_name = env
        .new_string(OVERLAY_CLASS.replace('/', "."))
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to create Android overlay helper class name: {error}")
        })?;
    let class_name = JObject::from(class_name);

    let class = env
        .call_method(activity, "getClass", "()Ljava/lang/Class;", &[])
        .and_then(|value| value.l())
        .and_then(|class| {
            env.call_method(
                &class,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .and_then(|value| value.l())
        })
        .and_then(|class_loader| {
            env.call_method(
                &class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&class_name)],
            )
            .and_then(|value| value.l())
        })
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!(
                "failed to load Android overlay helper class {}; include cranpose/android/java in the Android source set: {error}",
                OVERLAY_CLASS
            )
        })?;

    Ok(JClass::from(class))
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

fn push_overlay_event(event: AndroidOverlayWindowEvent) {
    overlay_events()
        .lock()
        .expect("overlay event queue poisoned")
        .push_back(event);
}

fn overlay_events() -> &'static Mutex<VecDeque<AndroidOverlayWindowEvent>> {
    static EVENTS: OnceLock<Mutex<VecDeque<AndroidOverlayWindowEvent>>> = OnceLock::new();
    EVENTS.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn native_window_from_surface(
    env: &mut JNIEnv<'_>,
    surface: JObject<'_>,
) -> Result<NativeWindow, String> {
    // SAFETY: The callback is invoked by the Java helper with the current JNI
    // environment and a live android.view.Surface from SurfaceHolder.
    unsafe { NativeWindow::from_surface(env.get_native_interface(), surface.as_raw()) }.ok_or_else(
        || {
            clear_pending_android_jni_exception(env);
            "Android overlay Surface did not provide an ANativeWindow".to_string()
        },
    )
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlayCreateFailed(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    message: JString<'_>,
) {
    let message = env
        .get_string(&message)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "Android overlay window creation failed".to_string());
    push_overlay_event(AndroidOverlayWindowEvent::CreateFailed(message));
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlaySurfaceChanged(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    surface: JObject<'_>,
    width: jint,
    height: jint,
) {
    match native_window_from_surface(&mut env, surface) {
        Ok(native_window) if width > 0 && height > 0 => {
            push_overlay_event(AndroidOverlayWindowEvent::SurfaceChanged {
                native_window,
                width: width as u32,
                height: height as u32,
            });
        }
        Ok(_) => {}
        Err(message) => push_overlay_event(AndroidOverlayWindowEvent::CreateFailed(message)),
    }
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlaySurfaceDestroyed(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    push_overlay_event(AndroidOverlayWindowEvent::SurfaceDestroyed);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeOverlayWindow_nativeOverlayPointer(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
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
    push_overlay_event(AndroidOverlayWindowEvent::Pointer { action, x, y });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_to_physical_rounds_by_density() {
        assert_eq!(logical_to_physical_px(10.4, 2.0), Ok(21));
        assert_eq!(logical_to_physical_px(-3.0, 3.0), Ok(-9));
    }

    #[test]
    fn logical_to_physical_rejects_non_finite() {
        assert!(logical_to_physical_px(f32::NAN, 2.0).is_err());
        assert!(logical_to_physical_px(12.0, f32::INFINITY).is_err());
        assert!(logical_to_physical_px(12.0, 0.0).is_err());
    }

    #[test]
    fn logical_dimension_to_physical_clamps_to_visible_pixel() {
        assert_eq!(logical_dimension_to_physical_px(0.1, 1.0), Ok(1));
    }

    #[test]
    fn overlay_options_validate_positive_size() {
        assert!(AndroidOverlayWindowOptions::new(100, 50).is_valid());
        assert!(!AndroidOverlayWindowOptions::new(0, 50).is_valid());
        assert!(!AndroidOverlayWindowOptions::new(100, 0).is_valid());
    }

    #[test]
    fn overlay_options_to_physical_bounds_uses_initial_position_and_size() {
        let options = AndroidOverlayWindowOptions::new(100, 50).with_position(-4, 8);
        let bounds = overlay_options_to_physical_bounds(options, 2.0).unwrap();

        assert_eq!(
            bounds,
            AndroidOverlayWindowBounds {
                width_px: 200,
                height_px: 100,
                x_px: -8,
                y_px: 16,
            }
        );
    }

    #[test]
    fn overlay_bounds_to_physical_uses_runtime_position_and_size() {
        let bounds =
            overlay_bounds_to_physical(Point::new(12.25, -4.5), Size::new(200.0, 80.0), 2.0)
                .unwrap();

        assert_eq!(
            bounds,
            AndroidOverlayWindowBounds {
                width_px: 400,
                height_px: 160,
                x_px: 25,
                y_px: -9,
            }
        );
    }
}
