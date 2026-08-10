//! Android `AccessibilityNodeProvider` bridge for the native canvas surface.
#![allow(unsafe_code)]

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};
use crate::android_jni::{clear_pending_android_jni_exception, with_android_activity_env};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::jfloat,
    EnvUnowned,
};
use std::sync::{Mutex, OnceLock};

static ACTIVATIONS: OnceLock<Mutex<Vec<(f32, f32)>>> = OnceLock::new();
static LOOP_WAKER: OnceLock<android_activity::AndroidAppWaker> = OnceLock::new();

pub(crate) fn set_waker(waker: android_activity::AndroidAppWaker) {
    let _ = LOOP_WAKER.set(waker);
}

fn activations() -> &'static Mutex<Vec<(f32, f32)>> {
    ACTIVATIONS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn drain_activations() -> Vec<(f32, f32)> {
    std::mem::take(
        &mut *activations()
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

fn encode_elements(elements: &[AccessibilityElement], density: f32) -> String {
    let density = density.max(f32::EPSILON);
    elements
        .iter()
        .map(|element| {
            let role = match element.role {
                AccessibilityRole::Button => 1,
                AccessibilityRole::StaticText => 2,
                AccessibilityRole::TextField => 3,
            };
            let (center_x, center_y) = element.bounds.center();
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                virtual_id(element.node_id),
                role,
                (element.bounds.x * density).round() as i32,
                (element.bounds.y * density).round() as i32,
                ((element.bounds.x + element.bounds.width) * density).round() as i32,
                ((element.bounds.y + element.bounds.height) * density).round() as i32,
                center_x,
                center_y,
                i32::from(element.clickable),
                escape(&element.label),
                escape(element.value.as_deref().unwrap_or("")),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn virtual_id(node_id: cranpose_core::NodeId) -> i32 {
    ((node_id as u64 & 0x7fff_ffff) as i32).max(1)
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
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

#[cfg(test)]
mod tests {
    use super::{escape, virtual_id};

    #[test]
    fn android_accessibility_wire_values_escape_record_delimiters() {
        assert_eq!(escape("A%\tB\nC\r"), "A%25%09B%0AC%0D");
    }

    #[test]
    fn android_virtual_ids_are_stable_copies_of_cranpose_node_ids() {
        assert_eq!(virtual_id(42), 42);
        assert_eq!(virtual_id(0), 1);
        assert_eq!(virtual_id(42), virtual_id(42));
    }
}
