//! Android `AccessibilityNodeProvider` bridge for the native canvas surface.
#![allow(unsafe_code)]

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
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
static LOOP_WAKER: Mutex<Option<android_activity::AndroidAppWaker>> = Mutex::new(None);
/// Whether Android reports any assistive technology as active, pushed by
/// `CranposeActivity` through `nativeOnAccessibilityStateChanged`. The frame
/// loop samples it every iteration; `false` until the activity's first push,
/// which happens in `onCreate` before the first frame can need it.
static PLATFORM_ACCESSIBILITY_ENABLED: AtomicBool = AtomicBool::new(false);

/// `CRANPOSE_A11Y_SYNC` (property `debug.cranpose.a11y_sync`) A/B override:
/// `0` forces the bridge off, `1` forces it on regardless of what the platform
/// reports, anything else defers to `AccessibilityManager`.
fn accessibility_sync_override() -> Option<bool> {
    static OVERRIDE: OnceLock<Option<bool>> = OnceLock::new();
    *OVERRIDE.get_or_init(|| match std::env::var("CRANPOSE_A11Y_SYNC").as_deref() {
        Ok("0") | Ok("false") | Ok("off") => Some(false),
        Ok("1") | Ok("true") | Ok("on") => Some(true),
        _ => None,
    })
}

/// The bridge state the policy should follow this frame.
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

pub(crate) fn drain_activations() -> Vec<(f32, f32)> {
    std::mem::take(
        &mut *activations()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// `(virtual view id, index into that element's custom action list)` pairs a
/// screen reader asked for since the last frame.
pub(crate) fn drain_custom_actions() -> Vec<(i32, usize)> {
    std::mem::take(
        &mut *custom_actions()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
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
        // Assistive technology just arrived: whatever tree it can see on the
        // Java side is from a previous activation. Republish unconditionally.
        *seen_revision = None;
    }
    // The policy gate comes before any tree walk: the snapshot + encode +
    // JNI hop costs ~6.5 ms of a 16.7 ms frame on a 2018 SoC, so with no
    // assistive technology listening (or the throttle window still closed)
    // the frame must pay one comparison and nothing else.
    let now = std::time::Instant::now();
    if !policy.try_begin_publish(now) {
        return Ok(());
    }
    let Some(elements) = accessibility::snapshot_if_changed(shell, seen_revision) else {
        return Ok(());
    };
    if elements == *previous {
        return Ok(());
    }
    *previous = elements;
    policy.note_published(now);
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

/// `AccessibilityManager` state pushed by the activity: once at `onCreate`
/// and again whenever assistive technology starts or stops. The wake matters
/// on enable — the frame loop may be idle, and the republish must not wait
/// for the next app-driven frame.
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

/// A screen reader picked one of the node's custom actions.
///
/// Only the identity travels: which virtual view, and which action in the list
/// that view published. Resolving that to a handler happens on the frame loop
/// against the live semantics tree, so nothing here holds app state.
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
