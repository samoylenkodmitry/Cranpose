#![allow(unsafe_code)]

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
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
static TEXT_REQUESTS: OnceLock<Mutex<Vec<(i32, String)>>> = OnceLock::new();
static SCROLL_REQUESTS: OnceLock<Mutex<Vec<(i32, bool)>>> = OnceLock::new();
static EXPAND_REQUESTS: OnceLock<Mutex<Vec<(i32, bool)>>> = OnceLock::new();
static LONG_CLICK_REQUESTS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();
static DISMISS_REQUESTS: OnceLock<Mutex<Vec<i32>>> = OnceLock::new();
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

fn text_requests() -> &'static Mutex<Vec<(i32, String)>> {
    TEXT_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn scroll_requests() -> &'static Mutex<Vec<(i32, bool)>> {
    SCROLL_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn expand_requests() -> &'static Mutex<Vec<(i32, bool)>> {
    EXPAND_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn long_click_requests() -> &'static Mutex<Vec<i32>> {
    LONG_CLICK_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn dismiss_requests() -> &'static Mutex<Vec<i32>> {
    DISMISS_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
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

/// Text TalkBack or a voice tool handed to text fields, as virtual view ids.
pub(crate) fn drain_text_requests() -> Vec<(i32, String)> {
    std::mem::take(
        &mut *text_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Containers TalkBack asked to page, as virtual view ids, and which way.
pub(crate) fn drain_scroll_requests() -> Vec<(i32, bool)> {
    std::mem::take(
        &mut *scroll_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Controls TalkBack asked to open or to close, as virtual view ids.
pub(crate) fn drain_expand_requests() -> Vec<(i32, bool)> {
    std::mem::take(
        &mut *expand_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Controls TalkBack asked for a long press on, as virtual view ids.
pub(crate) fn drain_long_click_requests() -> Vec<i32> {
    std::mem::take(
        &mut *long_click_requests()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Controls TalkBack asked to send away, as virtual view ids.
pub(crate) fn drain_dismiss_requests() -> Vec<i32> {
    std::mem::take(
        &mut *dismiss_requests()
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
        announcements.extend(accessibility::pane_title_announcements(previous, elements));
    }
    speak(app, announcements)?;
    let Some(elements) = elements else {
        return Ok(());
    };
    let changed = accessibility::spoken_changes(previous, &elements);
    *previous = elements;
    let payload = encode_elements(previous, &changed, density);
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

/// TalkBack or a voice tool handed a text field new text. The frame loop
/// resolves the field against the live semantics tree.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilitySetText(
    mut env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
    text: JString<'_>,
) {
    let Outcome::Ok(text) = env
        .with_env(|env| -> jni::errors::Result<String> { text.try_to_string(env) })
        .into_outcome()
    else {
        return;
    };
    text_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((virtual_id, text));
    wake_loop();
}

/// TalkBack asked a container for its next or previous page. The frame loop
/// resolves the container against the live semantics tree and pages it.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityScroll(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
    forward: jboolean,
) {
    scroll_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((virtual_id, forward));
    wake_loop();
}

/// TalkBack asked a control to open or to close. The frame loop resolves the
/// control against the live semantics tree, as a custom action does.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityExpand(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
    open: jboolean,
) {
    expand_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((virtual_id, open));
    wake_loop();
}

/// TalkBack asked a control for its long press. The frame loop resolves the
/// control against the live semantics tree, as a custom action does.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityLongClick(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
) {
    long_click_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(virtual_id);
    wake_loop();
}

/// TalkBack asked a control to go away. The frame loop resolves the control
/// against the live semantics tree, as a custom action does.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityDismiss(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    virtual_id: jint,
) {
    dismiss_requests()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(virtual_id);
    wake_loop();
}
