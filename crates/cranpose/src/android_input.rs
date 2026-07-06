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

#[cfg(test)]
mod tests {
    use super::*;

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
