use super::{
    jni::{clear_pending_android_jni_exception, with_android_activity_env},
    overlay_window,
};

pub(super) fn set(
    app: &android_activity::AndroidApp,
    width_px: i32,
    height_px: i32,
) -> Result<(), String> {
    use jni::{jni_sig, jni_str, objects::JValue};

    with_android_activity_env(app, |env, activity| {
        let class = overlay_window::find_android_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                jni_str!("setActivityWindowLayout"),
                jni_sig!("(Landroid/app/Activity;II)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(width_px),
                    JValue::Int(height_px),
                ],
            )
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
