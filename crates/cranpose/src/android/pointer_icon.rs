use cranpose_ui::{CursorIcon, PointerIcon};
use jni::{
    jni_sig, jni_str,
    objects::{JObject, JValue},
};

use super::jni::{clear_pending_android_jni_exception, with_android_activity_env};

fn system_type(icon: CursorIcon) -> i32 {
    match icon {
        CursorIcon::Pointer => 1002,
        CursorIcon::Help => 1003,
        CursorIcon::Wait | CursorIcon::Progress => 1004,
        CursorIcon::Crosshair => 1007,
        CursorIcon::Text => 1008,
        CursorIcon::VerticalText => 1009,
        CursorIcon::Alias => 1010,
        CursorIcon::Copy => 1011,
        CursorIcon::NoDrop | CursorIcon::NotAllowed => 1012,
        CursorIcon::Move | CursorIcon::AllScroll => 1013,
        CursorIcon::EwResize
        | CursorIcon::ColResize
        | CursorIcon::EResize
        | CursorIcon::WResize => 1014,
        CursorIcon::NsResize
        | CursorIcon::RowResize
        | CursorIcon::NResize
        | CursorIcon::SResize => 1015,
        CursorIcon::NeswResize | CursorIcon::NeResize | CursorIcon::SwResize => 1016,
        CursorIcon::NwseResize | CursorIcon::NwResize | CursorIcon::SeResize => 1017,
        CursorIcon::ZoomIn => 1018,
        CursorIcon::ZoomOut => 1019,
        CursorIcon::Grab => 1020,
        CursorIcon::Grabbing => 1021,
        _ => 1000,
    }
}
pub(super) fn apply(app: &android_activity::AndroidApp, icon: &PointerIcon) {
    let result = with_android_activity_env(app, |env, activity| {
        let (kind, id, width, height, x, y, pixels) = match icon {
            PointerIcon::System(icon) => (system_type(*icon), 0, 0, 0, 0., 0., None),
            PointerIcon::Custom(icon) => (
                -1,
                icon.id() as i64,
                icon.image().width() as i32,
                icon.image().height() as i32,
                icon.hotspot_x() as f32,
                icon.hotspot_y() as f32,
                Some(icon.image().rgba8_pixels()),
            ),
        };
        let data = match pixels {
            Some(pixels) => JObject::from(
                env.byte_array_from_slice(&pixels)
                    .map_err(|e| e.to_string())?,
            ),
            None => JObject::null(),
        };
        env.call_method(
            &activity,
            jni_str!("cranposeSetPointerIcon"),
            jni_sig!("(IJ[BIIFF)V"),
            &[
                JValue::Int(kind),
                JValue::Long(id),
                JValue::Object(&data),
                JValue::Int(width),
                JValue::Int(height),
                JValue::Float(x),
                JValue::Float(y),
            ],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
        .inspect_err(|_| clear_pending_android_jni_exception(env))
    });
    if let Err(error) = result {
        log::warn!("Android pointer icon: {error}");
    }
}
