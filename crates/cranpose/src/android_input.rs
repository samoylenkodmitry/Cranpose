use cranpose_app_shell::PointerSource;

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidToolKind {
    Finger,
    Mouse,
    Stylus,
    Indeterminate,
}

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidSourceKind {
    Touchscreen,
    Stylus,
    Mouse,
    Other,
}

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
const ANDROID_SOURCE_TOUCHSCREEN: u32 = 0x0000_1002;
#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
const ANDROID_SOURCE_MOUSE: u32 = 0x0000_2002;
#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
const ANDROID_SOURCE_STYLUS: u32 = 0x0000_4002;
#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
const ANDROID_SOURCE_MOUSE_RELATIVE: u32 = 0x0002_0004;

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
pub(crate) fn android_source_kind(source_bits: u32) -> AndroidSourceKind {
    if source_bits & ANDROID_SOURCE_TOUCHSCREEN == ANDROID_SOURCE_TOUCHSCREEN {
        AndroidSourceKind::Touchscreen
    } else if source_bits & ANDROID_SOURCE_STYLUS == ANDROID_SOURCE_STYLUS {
        AndroidSourceKind::Stylus
    } else if source_bits & ANDROID_SOURCE_MOUSE == ANDROID_SOURCE_MOUSE
        || source_bits & ANDROID_SOURCE_MOUSE_RELATIVE == ANDROID_SOURCE_MOUSE_RELATIVE
    {
        AndroidSourceKind::Mouse
    } else {
        AndroidSourceKind::Other
    }
}

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
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

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
pub(crate) fn is_rotary_encoder_source(source_bits: u32) -> bool {
    source_bits & ANDROID_SOURCE_ROTARY_ENCODER == ANDROID_SOURCE_ROTARY_ENCODER
}

#[cfg_attr(
    not(all(feature = "android", feature = "renderer-wgpu", target_os = "android")),
    allow(dead_code, reason = "only the Android backend reads Android input")
)]
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
