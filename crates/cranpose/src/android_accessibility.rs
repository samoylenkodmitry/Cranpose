#![allow(unsafe_code)]

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::{jboolean, jfloat, jint},
};

use crate::{
    accessibility::{self, AccessibilityElement},
    accessibility_publish_policy::AccessibilityPublishPolicy,
    android_accessibility_wire::encode_elements,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
};

static ACTIVATIONS: OnceLock<Mutex<Vec<(f32, f32)>>> = OnceLock::new();
static CUSTOM_ACTIONS: OnceLock<Mutex<Vec<(i32, usize)>>> = OnceLock::new();
static FOCUS_REQUESTS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();
static VALUE_REQUESTS: OnceLock<Mutex<Vec<(i32, f32)>>> = OnceLock::new();
static LOOP_WAKER: Mutex<Option<android_activity::AndroidAppWaker>> = Mutex::new(None);
static PLATFORM_ACCESSIBILITY_ENABLED: AtomicBool = AtomicBool::new(false);

fn accessibility_sync_override() -> Option<bool> {
    static OVERRIDE: OnceLock<Option<bool>> = OnceLock::new();
    *OVERRIDE.get_or_init(|| match std::env::var("CRANPOSE_A11Y_SYNC").as_deref() {
        Ok("0") | Ok("false") | Ok("off") => Some(false),
        Ok("1") | Ok("true") | Ok("on") => Some(true),
        _ => None,
    })
}

fn accessibility_bridge_enabled() -> bool {
    accessibility_sync_override()
        .unwrap_or_else(|| PLATFORM_ACCESSIBILITY_ENABLED.load(Ordering::Relaxed))
}

pub(crate) fn set_waker(waker: android_activity::AndroidAppWaker) {
    *LOOP_WAKER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(waker);
}

fn wake_loop() {
    let waker = LOOP_WAKER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(waker) = waker {
        waker.wake();
    }
}

fn activations() -> &'static Mutex<Vec<(f32, f32)>> {
    ACTIVATIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn custom_actions() -> &'static Mutex<Vec<(i32, usize)>> {
    CUSTOM_ACTIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn focus_requests() -> &'static Mutex<Vec<i32>> {
    FOCUS_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn value_requests() -> &'static Mutex<Vec<(i32, f32)>> {
    VALUE_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn drain_activations() -> Vec<(f32, f32)> {
    std::mem::take(
        &mut *activations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

pub(crate) fn drain_custom_actions() -> Vec<(i32, usize)> {
    std::mem::take(
        &mut *custom_actions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// The virtual view ids TalkBack put its cursor on since the last frame.
pub(crate) fn drain_focus_requests() -> Vec<i32> {
    std::mem::take(
        &mut *focus_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Values TalkBack asked adjustable controls to take, as virtual view ids.
pub(crate) fn drain_value_requests() -> Vec<(i32, f32)> {
    std::mem::take(
        &mut *value_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Hands TalkBack text to read out at once, with no control to move to.
/// Android reads a live region set on a virtual view only through its host, so
/// a live region change reaches the user the same way an app announcement
/// does: as one spoken line.
fn speak(
    app: &android_activity::AndroidApp,
    announcements: Vec<cranpose_ui::Announcement>,
) -> Result<(), String> {
    if announcements.is_empty() {
        return Ok(());
    }
    let text = announcements
        .into_iter()
        .map(|announcement| announcement.text)
        .collect::<Vec<_>>()
        .join(". ");
    with_android_activity_env(app, |env, activity| {
        let text = env.new_string(text).map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to encode an accessibility announcement: {error}")
        })?;
        let text = JObject::from(text);
        env.call_method(
            &activity,
            jni_str!("cranposeAnnounceForAccessibility"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&text)],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to read out an accessibility announcement: {error}")
        })?;
        Ok(())
    })
}

pub(crate) fn sync(
    app: &android_activity::AndroidApp,
    shell: &mut AppShell<WgpuRenderer>,
    density: f32,
    previous: &mut Vec<AccessibilityElement>,
    seen_revision: &mut Option<u64>,
    policy: &mut AccessibilityPublishPolicy,
) -> Result<(), String> {
    if policy.update_enabled(accessibility_bridge_enabled()) {
        *seen_revision = None;
    }
    let mut announcements = accessibility::drain_app_announcements();
    let now = std::time::Instant::now();
    let elements = if policy.try_begin_publish(now) {
        accessibility::snapshot_if_changed(shell, seen_revision)
    } else {
        None
    };
    let elements = elements.filter(|elements| elements != previous);
    if let Some(elements) = &elements {
        announcements.extend(accessibility::live_region_announcements(previous, elements));
    }
    speak(app, announcements)?;
    let Some(elements) = elements else {
        return Ok(());
    };
    *previous = elements;
    let payload = encode_elements(previous, density);
    with_android_activity_env(app, |env, activity| {
        let payload = env.new_string(payload).map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to encode Android accessibility tree: {error}")
        })?;
        let payload = JObject::from(payload);
        env.call_method(
            &activity,
            jni_str!("cranposeSetAccessibilityElements"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&payload)],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to publish Android accessibility tree: {error}")
        })?;
        Ok(())
    })
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityActivate(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    x: jfloat,
    y: jfloat,
) {
    activations()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((x, y));
    wake_loop();
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityStateChanged(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    enabled: jboolean,
) {
    let previous = PLATFORM_ACCESSIBILITY_ENABLED.swap(enabled, Ordering::Relaxed);
    if previous != enabled {
        wake_loop();
    }
}

/// TalkBack landed its cursor on a virtual view; the frame loop moves app
/// focus to match, so the reader and the app agree on what holds focus.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityFocus(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
) {
    focus_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(virtual_id);
    wake_loop();
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityCustomAction(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
    action_index: jint,
) {
    if action_index < 0 {
        return;
    }
    custom_actions()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((virtual_id, action_index as usize));
    wake_loop();
}

/// TalkBack moved the value of an adjustable control. Only the identity and
/// the value cross back; the frame loop resolves the control against the live
/// semantics tree, as a custom action does.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilitySetProgress(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
    value: jfloat,
) {
    value_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((virtual_id, value));
    wake_loop();
}
