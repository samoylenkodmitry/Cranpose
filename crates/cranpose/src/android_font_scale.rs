//! Reads Android's system font-size setting, and the conversion behind it.
//!
//! `Configuration.fontScale` is the multiplier behind Settings → Display →
//! Font size. It is not in the NDK's `AConfiguration`, so it comes over JNI,
//! and it changes while the app runs — the platform delivers a configuration
//! change rather than restarting the process — so it is read again on every
//! `ConfigChanged`.
//!
//! The multiplier alone is not enough. Since Android 14 a size in `sp` is not
//! `sp * fontScale`: above a threshold setting the platform runs it through a
//! piecewise-linear table, so small text grows by the whole setting and large
//! text grows by less (see [`cranpose_ui::font_scale`]). The table is not
//! public API, but the conversion is: `TypedValue.applyDimension` performs it.
//! So the conversion is sampled rather than reimplemented — no table is copied
//! into this repository, and a device shipping its own answers for itself.
//!
//! Wear OS quality guideline WO-V1 asks that text follow this setting, and an
//! app cannot honour it if the framework never tells it what the setting is.

use cranpose_ui::FontScaleCurve;
use jni::objects::JObject;
use jni::{jni_sig, jni_str};
use std::cell::Cell;

/// `TypedValue.COMPLEX_UNIT_SP`.
const COMPLEX_UNIT_SP: i32 = 2;

/// The sizes the platform's conversion is sampled at, in sp.
///
/// Dense over the range text is actually set in and sparse above it, because
/// the platform's own table bends only in the former. The samples are reduced
/// to the points that bend before they are stored, so this ladder costs knots
/// only where the curve has them — a device whose conversion is a straight line
/// comes back as two points.
const SAMPLE_SP: [f32; 36] = [
    1.0, 2.0, 4.0, 6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0,
    21.0, 22.0, 23.0, 24.0, 26.0, 28.0, 30.0, 32.0, 36.0, 40.0, 48.0, 56.0, 64.0, 80.0, 96.0,
    112.0, 128.0, 160.0, 200.0,
];

thread_local! {
    /// Last conversion read from the platform. The geometry update that hands
    /// it to the shell runs from call sites that do not all hold an
    /// `AndroidApp`, and a JNI round trip does not belong on a per-frame path
    /// anyway, so it is refreshed at startup and on configuration changes and
    /// read from here in between.
    static FONT_SCALE: Cell<FontScaleCurve> = const { Cell::new(FontScaleCurve::linear(1.0)) };
}

/// The most recently read conversion, the identity until the first read.
pub(crate) fn font_scale_curve() -> FontScaleCurve {
    FONT_SCALE.with(Cell::get)
}

/// Re-reads the setting and its conversion, and returns true when either moved.
///
/// A failure is not fatal: the last value stands, which at worst is the `1.0`
/// the app behaved as before the setting was readable at all.
pub(crate) fn refresh_font_scale(app: &android_activity::AndroidApp) -> bool {
    match query_font_scale(app) {
        Ok(curve) => {
            let previous = font_scale_curve();
            FONT_SCALE.with(|cell| cell.set(curve));
            previous != curve
        }
        Err(error) => {
            log::warn!("[android-font-scale] could not read Configuration.fontScale: {error}");
            false
        }
    }
}

fn query_font_scale(app: &android_activity::AndroidApp) -> Result<FontScaleCurve, String> {
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

        let metrics = env
            .call_method(
                &resources,
                jni_str!("getDisplayMetrics"),
                jni_sig!("()Landroid/util/DisplayMetrics;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Resources.getDisplayMetrics", error))?;
        let density = env
            .get_field(&metrics, jni_str!("density"), jni_sig!("F"))
            .and_then(|value| value.f())
            .map_err(|error| describe(env, "DisplayMetrics.density", error))?;
        if !density.is_finite() || density <= 0.0 {
            return Err(format!("DisplayMetrics.density was {density}"));
        }

        match sample_curve(env, &metrics, scale, density) {
            Ok(curve) => Ok(curve),
            Err(error) => {
                // The setting is still known; only the shape of the conversion
                // is not. Multiplying is what every Android before 14 does, so
                // it is the right thing to fall back to rather than to refuse.
                log::warn!("[android-font-scale] sampling TypedValue.applyDimension: {error}");
                Ok(FontScaleCurve::linear(scale))
            }
        }
    })
}

/// Asks `TypedValue.applyDimension` what each sample size comes to, and builds
/// the curve through the answers.
fn sample_curve(
    env: &mut jni::Env<'_>,
    metrics: &JObject<'_>,
    scale: f32,
    density: f32,
) -> Result<FontScaleCurve, String> {
    let class = env
        .find_class(jni_str!("android/util/TypedValue"))
        .map_err(|error| {
            crate::android_jni::clear_pending_android_jni_exception(env);
            format!("find android.util.TypedValue: {error}")
        })?;
    let mut samples = [(0.0f32, 0.0f32); SAMPLE_SP.len()];
    for (slot, sp) in samples.iter_mut().zip(SAMPLE_SP) {
        let px = env
            .call_static_method(
                &class,
                jni_str!("applyDimension"),
                jni_sig!("(IFLandroid/util/DisplayMetrics;)F"),
                &[
                    jni::objects::JValue::Int(COMPLEX_UNIT_SP),
                    jni::objects::JValue::Float(sp),
                    jni::objects::JValue::Object(metrics),
                ],
            )
            .and_then(|value| value.f())
            .map_err(|error| {
                crate::android_jni::clear_pending_android_jni_exception(env);
                format!("TypedValue.applyDimension({sp}sp): {error}")
            })?;
        *slot = (sp, px / density);
    }
    Ok(FontScaleCurve::from_samples(scale, &samples))
}
