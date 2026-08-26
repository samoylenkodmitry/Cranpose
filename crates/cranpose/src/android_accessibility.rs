//! Android `AccessibilityNodeProvider` bridge for the native canvas surface.
#![allow(unsafe_code)]

use std::sync::{Mutex, OnceLock};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    EnvUnowned, jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::{jfloat, jint},
};

use crate::{
    accessibility::{self, AccessibilityElement},
    android_accessibility_wire::encode_elements,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
};

static ACTIVATIONS: OnceLock<Mutex<Vec<(f32, f32)>>> = OnceLock::new();
static CUSTOM_ACTIONS: OnceLock<Mutex<Vec<(i32, usize)>>> = OnceLock::new();
static LOOP_WAKER: Mutex<Option<android_activity::AndroidAppWaker>> = Mutex::new(None);

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
