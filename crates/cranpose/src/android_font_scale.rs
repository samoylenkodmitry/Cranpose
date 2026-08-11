//! Reads Android's system font-size setting.
//!
//! `Configuration.fontScale` is the multiplier behind Settings → Display →
//! Font size. It is not in the NDK's `AConfiguration`, so it comes over JNI,
//! and it changes while the app runs — the platform delivers a configuration
//! change rather than restarting the process — so it is read again on every
//! `ConfigChanged`.
//!
//! Wear OS quality guideline WO-V1 asks that text follow this setting, and an
//! app cannot honour it if the framework never tells it what the setting is.

use jni::{jni_sig, jni_str};
use std::cell::Cell;

thread_local! {
    /// Last value read from the platform. The geometry update that hands it to
    /// the shell runs from call sites that do not all hold an `AndroidApp`,
    /// and a JNI round trip does not belong on a per-frame path anyway, so the
    /// value is refreshed at startup and on configuration changes and read
    /// from here in between.
    static FONT_SCALE: Cell<f32> = const { Cell::new(1.0) };
}

/// The most recently read system font scale, `1.0` until the first read.
pub(crate) fn font_scale() -> f32 {
    FONT_SCALE.with(Cell::get)
}

/// Re-reads `Configuration.fontScale` and returns true when it changed.
///
/// A failure is not fatal: the last value stands, which at worst is the `1.0`
/// the app behaved as before the setting was readable at all.
pub(crate) fn refresh_font_scale(app: &android_activity::AndroidApp) -> bool {
    match query_font_scale(app) {
        Ok(scale) => {
            let previous = font_scale();
            FONT_SCALE.with(|cell| cell.set(scale));
            (scale - previous).abs() > f32::EPSILON
        }
        Err(error) => {
            log::warn!("[android-font-scale] could not read Configuration.fontScale: {error}");
            false
        }
    }
}

fn query_font_scale(app: &android_activity::AndroidApp) -> Result<f32, String> {
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        let describe = |env: &mut jni::Env<'_>, what: &str, error: jni::errors::Error| {
            crate::android_jni::clear_pending_android_jni_exception(env);
            format!("{what} failed: {error}")
        };
        let resources = env
            .call_method(
                &activity,
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Activity.getResources", error))?;
        let configuration = env
            .call_method(
                &resources,
                jni_str!("getConfiguration"),
                jni_sig!("()Landroid/content/res/Configuration;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Resources.getConfiguration", error))?;
        let scale = env
            .get_field(&configuration, jni_str!("fontScale"), jni_sig!("F"))
            .and_then(|value| value.f())
            .map_err(|error| describe(env, "Configuration.fontScale", error))?;
        Ok(scale)
    })
}
