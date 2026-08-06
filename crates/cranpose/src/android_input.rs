//! Host-testable classification of Android pointer device sources.
//!
//! An Android `MotionEvent` exposes both a per-pointer *tool type*
//! (`getToolType`) and a whole-event input *source* class (`getSource`). The
//! tool type is the most specific signal, but a number of devices and the
//! Android emulator report `TOOL_TYPE_UNKNOWN` for genuine finger touches. When
//! that happens the pointer must still be classified as touch (from the
//! touchscreen source class) or the finger selection/cursor handles — which are
//! only shown for touch/stylus input — never appear.
//!
//! The real `android_activity` `ToolType`/`Source` enums only exist on the
//! android target, so this module works on small host-visible mirror enums and
//! is exercised by ordinary unit tests. `android.rs` maps the platform enums
//! onto these kinds at the boundary.

use cranpose_app_shell::PointerSource;

/// Per-pointer tool category, mirroring the `android_activity` `ToolType`
/// variants that matter for source classification.
#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidToolKind {
    Finger,
    Mouse,
    Stylus,
    /// A tool type that does not by itself identify the device (Unknown, Palm,
    /// or a variant added by a newer Android version).
    Indeterminate,
}

/// Whole-event input source class, mirroring the `android_activity` `Source`
/// variants that matter for source classification.
#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AndroidSourceKind {
    Touchscreen,
    Stylus,
    Mouse,
    /// A source class that does not identify a direct pointing device.
    Other,
}

/// Resolves an Android pointer's [`PointerSource`] from its tool type and the
/// event's input source class.
///
/// The tool type wins when it identifies the device; otherwise the source class
/// is consulted so a touchscreen press whose tool type is unreported is still
/// treated as touch (the finger selection/cursor handles depend on it).
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

/// Android's `AINPUT_SOURCE_ROTARY_ENCODER`.
///
/// Wear OS reports the Pixel Watch crown and the Galaxy Watch rotating bezel
/// through this source class. It is a *bit flag* on the event's source value,
/// not an equality test — the NDK's own idiom is
/// `AInputEvent_getSource(event) & AINPUT_SOURCE_ROTARY_ENCODER`.
pub(crate) const ANDROID_SOURCE_ROTARY_ENCODER: u32 = 0x0040_0000;

/// Returns whether an Android input source value identifies a rotary encoder.
///
/// Takes the raw source bits so it works for source classes newer than the
/// `android_activity` `Source` enum knows about (the enum is runtime-extensible
/// and falls back to an opaque unknown variant).
#[cfg_attr(not(all(feature = "android", target_os = "android")), allow(dead_code))]
pub(crate) fn is_rotary_encoder_source(source_bits: u32) -> bool {
    source_bits & ANDROID_SOURCE_ROTARY_ENCODER == ANDROID_SOURCE_ROTARY_ENCODER
}

/// Fallback pixels-per-detent factor for rotary input at a given display
/// density.
///
/// Android's real factor is
/// `ViewConfiguration.getScaledVerticalScrollFactor()`, which resolves the
/// theme's `listPreferredItemHeight` and therefore needs a JVM `Context`.
/// Cranpose's Android input path holds no JNI handle, so it approximates the
/// platform default (`DEFAULT_ROTARY_SCROLL_FACTOR_DP` dp) and lets the host
/// override it with the exact value via
/// `AppShell::set_rotary_scroll_factor`.
///
/// A non-finite or non-positive density falls back to 1.0 so the factor stays
/// usable rather than poisoning every subsequent scroll offset with NaN.
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
        // Wear devices routinely OR the rotary class with other bits; an
        // equality test would miss those and deliver nothing at all.
        assert!(is_rotary_encoder_source(
            ANDROID_SOURCE_ROTARY_ENCODER | 0x0000_0101
        ));
    }

    #[test]
    fn non_rotary_sources_are_rejected() {
        // Touchscreen, mouse, and "no source" must not be treated as rotary or
        // ordinary scrolls would be duplicated as crown turns.
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
        // Regression guard: devices/emulators that report an unknown tool type
        // for a finger must still classify as touch, or the finger
        // selection/cursor handles (touch-only) never appear.
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
