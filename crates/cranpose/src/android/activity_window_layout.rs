use super::{
    jni::{clear_pending_android_jni_exception, with_android_activity_env},
    overlay_window,
};

pub(super) enum ActivityWindowLayout {
    Exact(i32, i32),
    System,
}

pub(super) fn set(
    app: &android_activity::AndroidApp,
    layout: ActivityWindowLayout,
) -> Result<(), String> {
    use jni::{jni_sig, jni_str, objects::JValue};

    with_android_activity_env(app, |env, activity| {
        let class = overlay_window::find_android_overlay_class(env, &activity)?;
        let result = match layout {
            ActivityWindowLayout::Exact(width, height) => env.call_static_method(
                class,
                jni_str!("setActivityWindowLayout"),
                jni_sig!("(Landroid/app/Activity;II)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(width),
                    JValue::Int(height),
                ],
            ),
            ActivityWindowLayout::System => env.call_static_method(
                class,
                jni_str!("restoreActivityWindowLayout"),
                jni_sig!("(Landroid/app/Activity;)I"),
                &[JValue::Object(&activity)],
            ),
        }
        .and_then(jni::JValueOwned::i)
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to request Android window layout: {error}")
        })?;

        match result {
            0 => Ok(()),
            code => Err(format!(
                "Android window layout request failed with code {code}"
            )),
        }
    })
}
