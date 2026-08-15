//! Android `AccessibilityNodeProvider` bridge for the native canvas surface.
#![allow(unsafe_code)]

use crate::accessibility::{self, AccessibilityElement};
use crate::android_accessibility_wire::encode_elements;
use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::{jfloat, jint},
    EnvUnowned,
};
use std::sync::{Mutex, OnceLock};

static ACTIVATIONS: OnceLock<Mutex<Vec<(f32, f32)>>> = OnceLock::new();
static CUSTOM_ACTIONS: OnceLock<Mutex<Vec<(i32, usize)>>> = OnceLock::new();
static LOOP_WAKER: OnceLock<android_activity::AndroidAppWaker> = OnceLock::new();

pub(crate) fn set_waker(waker: android_activity::AndroidAppWaker) {
    let _ = LOOP_WAKER.set(waker);
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
) -> Result<(), String> {
    let Some(elements) = accessibility::snapshot_if_changed(shell, seen_revision) else {
        return Ok(());
    };
    if elements == *previous {
        return Ok(());
    }
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
#[no_mangle]
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
    if let Some(waker) = LOOP_WAKER.get() {
        waker.wake();
    }
}

/// A screen reader picked one of the node's custom actions.
///
/// Only the identity travels: which virtual view, and which action in the list
/// that view published. Resolving that to a handler happens on the frame loop
/// against the live semantics tree, so nothing here holds app state.
#[doc(hidden)]
#[no_mangle]
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
    if let Some(waker) = LOOP_WAKER.get() {
        waker.wake();
    }
}
