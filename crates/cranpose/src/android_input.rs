use cranpose_app_shell::PointerSource;

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidToolKind {
    Finger,
    Mouse,
    Stylus,
    Indeterminate,
}

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidSourceKind {
    Touchscreen,
    Stylus,
    Mouse,
    Other,
}

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
pub(crate) fn resolve_pointer_source(
    tool: AndroidToolKind,
    source: AndroidSourceKind,
) -> PointerSource {
    match tool {
        AndroidToolKind::Finger => PointerSource::Touch,
        AndroidToolKind::Mouse => PointerSource::Mouse,
        AndroidToolKind::Stylus => PointerSource::Stylus,
        AndroidToolKind::Indeterminate => match source {
            AndroidSourceKind::Touchscreen => PointerSource::Touch,
            AndroidSourceKind::Stylus => PointerSource::Stylus,
            AndroidSourceKind::Mouse => PointerSource::Mouse,
            AndroidSourceKind::Other => PointerSource::Unknown,
        },
    }
}

pub(crate) const ANDROID_SOURCE_ROTARY_ENCODER: u32 = 0x0040_0000;

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
pub(crate) fn is_rotary_encoder_source(source_bits: u32) -> bool {
    source_bits & ANDROID_SOURCE_ROTARY_ENCODER == ANDROID_SOURCE_ROTARY_ENCODER
}

#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
pub(crate) fn android_rotary_scroll_factor(density: f32) -> f32 {
    let density = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    cranpose_app_shell::DEFAULT_ROTARY_SCROLL_FACTOR_DP * density
}

#[cfg(test)]
#[path = "tests/android_input_tests.rs"]
mod tests;
