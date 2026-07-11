//! Android backends for the cranpose service registry: haptics, share sheet,
//! notifications (with deep-links), network status, clipboard, and window
//! insets — the JNI counterparts of the capability hooks in
//! `dev.cranpose.android.CranposeActivity`.
//!
//! Rust → Java goes through the activity over JNI (each Java method hops to
//! the UI thread itself). Java → Rust arrives on the Android UI thread via
//! the exported `Java_dev_cranpose_android_CranposeActivity_*` symbols below;
//! those must not touch the composition (which lives on the native-activity
//! thread), so they park values in atomics and wake the native loop, which
//! applies them via [`apply_pending_platform_signals`].
#![allow(unsafe_code)]

use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use cranpose_services::{
    push_notification_deeplink, set_platform_haptics, set_platform_network_monitor,
    set_platform_notifier, set_platform_share_sheet, HapticFeedback, Haptics, NetworkMonitor,
    NetworkStatus, Notifier, NotifyRequest, ShareContent, ShareError, ShareSheet,
};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

// --- Cross-thread signal parking (UI thread → native loop) -------------------

static NETWORK_ONLINE: AtomicBool = AtomicBool::new(true);
static NETWORK_METERED: AtomicBool = AtomicBool::new(false);

static INSETS_LEFT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_TOP_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_RIGHT_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_BOTTOM_PX: AtomicI32 = AtomicI32::new(0);
static INSETS_CHANGED: AtomicBool = AtomicBool::new(false);

/// Wakes the native event loop so parked signals are applied promptly.
static LOOP_WAKER: OnceLock<Mutex<Option<android_activity::AndroidAppWaker>>> = OnceLock::new();

fn wake_native_loop() {
    if let Some(waker) = LOOP_WAKER.get() {
        if let Ok(waker) = waker.lock() {
            if let Some(waker) = waker.as_ref() {
                waker.wake();
            }
        }
    }
}

/// Registers the Android service backends. Called once at startup with the
/// activity handle.
pub(crate) fn register(app: android_activity::AndroidApp) {
    let _ = LOOP_WAKER.set(Mutex::new(Some(app.create_waker())));
    set_platform_haptics(Rc::new(AndroidHaptics { app: app.clone() }));
    set_platform_share_sheet(Rc::new(AndroidShareSheet { app: app.clone() }));
    set_platform_notifier(Rc::new(AndroidNotifier { app: app.clone() }));
    set_platform_network_monitor(Rc::new(AndroidNetworkMonitor));
}

/// Applies signals parked by the UI-thread callbacks: window insets flow into
/// the platform environment (safe area), forcing a root render when they
/// changed. Called from the native event loop.
pub(crate) fn apply_pending_platform_signals(
    density: f32,
    shell: &mut Option<cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>>,
) {
    if INSETS_CHANGED.swap(false, Ordering::AcqRel) {
        let density = density.max(f32::EPSILON);
        let insets = cranpose_ui::EdgeInsets {
            left: INSETS_LEFT_PX.load(Ordering::Acquire) as f32 / density,
            top: INSETS_TOP_PX.load(Ordering::Acquire) as f32 / density,
            right: INSETS_RIGHT_PX.load(Ordering::Acquire) as f32 / density,
            bottom: INSETS_BOTTOM_PX.load(Ordering::Acquire) as f32 / density,
        };
        if crate::android::android_platform_env().set_safe_area(insets) {
            if let Some(shell) = shell {
                shell.request_root_render();
            }
        }
    }
}

// --- Haptics ------------------------------------------------------------------

struct AndroidHaptics {
    app: android_activity::AndroidApp,
}

impl Haptics for AndroidHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let kind: jint = match feedback {
            HapticFeedback::ImpactLight | HapticFeedback::Selection => 0,
            HapticFeedback::ImpactMedium => 1,
            HapticFeedback::ImpactHeavy => 2,
            HapticFeedback::Success => 3,
            HapticFeedback::Warning | HapticFeedback::Error => 4,
        };
        let result = with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeHaptic"),
                jni_sig!("(I)V"),
                &[JValue::Int(kind)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
        if let Err(error) = result {
            log::debug!("Android haptic failed: {error}");
        }
    }
}

// --- Share sheet ----------------------------------------------------------------

struct AndroidShareSheet {
    app: android_activity::AndroidApp,
}

impl ShareSheet for AndroidShareSheet {
    fn share(&self, content: ShareContent) -> Result<(), ShareError> {
        with_android_activity_env(&self.app, |env, activity| {
            let name = env
                .new_string(&content.file_name)
                .map_err(|error| error.to_string())?;
            let mime = env
                .new_string(&content.mime_type)
                .map_err(|error| error.to_string())?;
            let bytes = env
                .byte_array_from_slice(&content.bytes)
                .map_err(|error| error.to_string())?;
            let text = env
                .new_string(content.text.as_deref().unwrap_or(""))
                .map_err(|error| error.to_string())?;
            let name_obj: &JObject = name.as_ref();
            let mime_obj: &JObject = mime.as_ref();
            let bytes_obj: &JObject = bytes.as_ref();
            let text_obj: &JObject = text.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeShare"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;[BLjava/lang/String;)V"),
                &[
                    JValue::Object(name_obj),
                    JValue::Object(mime_obj),
                    JValue::Object(bytes_obj),
                    JValue::Object(text_obj),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        })
        .map_err(ShareError::Failed)
    }

    fn is_supported(&self) -> bool {
        true
    }
}

// --- Notifier -------------------------------------------------------------------

struct AndroidNotifier {
    app: android_activity::AndroidApp,
}

impl AndroidNotifier {
    fn call(&self, run: impl FnOnce(&mut jni::Env<'_>, JObject<'_>) -> Result<(), String>) {
        let result = with_android_activity_env(&self.app, |env, activity| run(env, activity));
        if let Err(error) = result {
            log::warn!("Android notifier call failed: {error}");
        }
    }
}

impl Notifier for AndroidNotifier {
    fn request_permission(&self) {
        self.call(|env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeNotifyRequestPermission"),
                jni_sig!("()V"),
                &[],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn notify(&self, request: NotifyRequest) {
        self.call(|env, activity| {
            let tag = env
                .new_string(&request.id)
                .map_err(|error| error.to_string())?;
            let title = env
                .new_string(&request.title)
                .map_err(|error| error.to_string())?;
            let body = env
                .new_string(&request.body)
                .map_err(|error| error.to_string())?;
            let deeplink = env
                .new_string(request.deeplink.as_deref().unwrap_or(""))
                .map_err(|error| error.to_string())?;
            let tag_obj: &JObject = tag.as_ref();
            let title_obj: &JObject = title.as_ref();
            let body_obj: &JObject = body.as_ref();
            let deeplink_obj: &JObject = deeplink.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeNotify"),
                jni_sig!(
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZLjava/lang/String;)V"
                ),
                &[
                    JValue::Object(tag_obj),
                    JValue::Object(title_obj),
                    JValue::Object(body_obj),
                    JValue::Bool(request.ongoing),
                    JValue::Object(deeplink_obj),
                ],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }

    fn cancel(&self, id: &str) {
        self.call(|env, activity| {
            let tag = env.new_string(id).map_err(|error| error.to_string())?;
            let tag_obj: &JObject = tag.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeNotifyCancel"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(tag_obj)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
    }
}

// --- Network monitor --------------------------------------------------------------

struct AndroidNetworkMonitor;

impl NetworkMonitor for AndroidNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: NETWORK_ONLINE.load(Ordering::Acquire),
            metered: NETWORK_METERED.load(Ordering::Acquire),
        }
    }
}

// --- Clipboard ---------------------------------------------------------------------

/// The Android system clipboard for the text-selection menu / `ClipboardManager`.
pub(crate) struct AndroidClipboard {
    pub(crate) app: android_activity::AndroidApp,
}

impl cranpose_ui::clipboard_session::PlatformClipboard for AndroidClipboard {
    fn write_text(&self, text: &str) {
        let result = with_android_activity_env(&self.app, |env, activity| {
            let value = env.new_string(text).map_err(|error| error.to_string())?;
            let value_obj: &JObject = value.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeClipboardSet"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(value_obj)],
            )
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
        if let Err(error) = result {
            log::warn!("Android clipboard write failed: {error}");
        }
    }

    fn read_text(&self) -> Option<String> {
        with_android_activity_env(&self.app, |env, activity| {
            let value = env
                .call_method(
                    &activity,
                    jni_str!("cranposeClipboardGet"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )
                .and_then(|value| value.l())
                .map_err(|error| {
                    clear_pending_android_jni_exception(env);
                    error.to_string()
                })?;
            let value = env
                .cast_local::<JString>(value)
                .map_err(|error| error.to_string())?;
            value.try_to_string(env).map_err(|error| error.to_string())
        })
        .ok()
        .filter(|text| !text.is_empty())
    }
}

// --- Java → Rust callbacks (Android UI thread) ---------------------------------------

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnNetworkStatus(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    online: jboolean,
    metered: jboolean,
) {
    NETWORK_ONLINE.store(online, Ordering::Release);
    NETWORK_METERED.store(metered, Ordering::Release);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnInsetsChanged(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
) {
    INSETS_LEFT_PX.store(left, Ordering::Release);
    INSETS_TOP_PX.store(top, Ordering::Release);
    INSETS_RIGHT_PX.store(right, Ordering::Release);
    INSETS_BOTTOM_PX.store(bottom, Ordering::Release);
    INSETS_CHANGED.store(true, Ordering::Release);
    wake_native_loop();
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeNotificationAction<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    deeplink: JString<'local>,
) {
    let deeplink = match env
        .with_env(|env| -> jni::errors::Result<String> { deeplink.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(deeplink) => deeplink,
        Outcome::Err(_) | Outcome::Panic(_) => return,
    };
    if !deeplink.is_empty() {
        push_notification_deeplink(deeplink);
        wake_native_loop();
    }
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFileSaved<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    ok: jboolean,
    error: JString<'local>,
) {
    let error = match env
        .with_env(|env| -> jni::errors::Result<String> { error.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(error) if !error.is_empty() => Some(error),
        _ => None,
    };
    crate::android_file_picker::resolve_pending_save(token, ok, error);
    wake_native_loop();
}
