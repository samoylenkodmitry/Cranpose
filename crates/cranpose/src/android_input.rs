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
mod tests {
    use super::*;

    #[test]
    fn rotary_encoder_source_is_detected_as_a_bit_flag() {
        assert!(is_rotary_encoder_source(ANDROID_SOURCE_ROTARY_ENCODER));
        assert!(is_rotary_encoder_source(
            ANDROID_SOURCE_ROTARY_ENCODER | 0x0000_0101
        ));
    }

    #[test]
    fn non_rotary_sources_are_rejected() {
        assert!(!is_rotary_encoder_source(0x0000_1002));
        assert!(!is_rotary_encoder_source(0x0000_2002));
        assert!(!is_rotary_encoder_source(0));
    }

    #[test]
    fn rotary_scroll_factor_scales_with_density() {
        let base = cranpose_app_shell::DEFAULT_ROTARY_SCROLL_FACTOR_DP;

        assert_eq!(android_rotary_scroll_factor(1.0), base);
        assert_eq!(android_rotary_scroll_factor(2.0), base * 2.0);
    }

    #[test]
    fn rotary_scroll_factor_rejects_unusable_densities() {
        let base = cranpose_app_shell::DEFAULT_ROTARY_SCROLL_FACTOR_DP;

        assert_eq!(android_rotary_scroll_factor(0.0), base);
        assert_eq!(android_rotary_scroll_factor(-2.0), base);
        assert_eq!(android_rotary_scroll_factor(f32::NAN), base);
        assert!(android_rotary_scroll_factor(f32::INFINITY).is_finite());
    }

    #[test]
    fn tool_type_wins_when_it_identifies_the_device() {
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Finger, AndroidSourceKind::Mouse),
            PointerSource::Touch
        );
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Mouse, AndroidSourceKind::Touchscreen),
            PointerSource::Mouse
        );
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Stylus, AndroidSourceKind::Other),
            PointerSource::Stylus
        );
    }

    #[test]
    fn unreported_tool_type_falls_back_to_touchscreen_source() {
        assert_eq!(
            resolve_pointer_source(
                AndroidToolKind::Indeterminate,
                AndroidSourceKind::Touchscreen
            ),
            PointerSource::Touch
        );
        assert!(PointerSource::Touch.is_touch_like());
    }

    #[test]
    fn unreported_tool_type_uses_the_source_class() {
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Indeterminate, AndroidSourceKind::Stylus),
            PointerSource::Stylus
        );
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Indeterminate, AndroidSourceKind::Mouse),
            PointerSource::Mouse
        );
        assert_eq!(
            resolve_pointer_source(AndroidToolKind::Indeterminate, AndroidSourceKind::Other),
            PointerSource::Unknown
        );
    }
}
