use std::cell::Cell;

use cranpose_ui::FontScaleCurve;
use jni::{jni_sig, jni_str, objects::JObject};

const COMPLEX_UNIT_SP: i32 = 2;

const SAMPLE_SP: [f32; 36] = [
    1.0, 2.0, 4.0, 6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0, 19.0, 20.0,
    21.0, 22.0, 23.0, 24.0, 26.0, 28.0, 30.0, 32.0, 36.0, 40.0, 48.0, 56.0, 64.0, 80.0, 96.0,
    112.0, 128.0, 160.0, 200.0,
];

thread_local! {
    static FONT_SCALE: Cell<FontScaleCurve> = const { Cell::new(FontScaleCurve::linear(1.0)) };
}

pub(crate) fn font_scale_curve() -> FontScaleCurve {
    FONT_SCALE.with(Cell::get)
}

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
                log::warn!("[android-font-scale] sampling TypedValue.applyDimension: {error}");
                Ok(FontScaleCurve::linear(scale))
            }
        }
    })
}

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
