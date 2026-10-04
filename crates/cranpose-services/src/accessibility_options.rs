//! The display options a person set in the system's accessibility settings:
//! larger text, less motion, less transparency, more contrast, bold text,
//! inverted colors. The framework applies them on its own; an app reads them
//! for what it draws itself.

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;
pub use cranpose_ui_graphics::accessibility::{
    AccessibilityOptions, platform_accessibility_options, set_platform_accessibility_options,
};

/// The options a composable reads: what the platform reported, unless a
/// [`ProvideAccessibilityOptions`] above it says otherwise.
pub fn local_accessibility_options() -> CompositionLocal<AccessibilityOptions> {
    crate::composition_locals::cached_local(
        |locals| &locals.accessibility_options,
        || compositionLocalOf(platform_accessibility_options),
    )
}

/// Gives the content below it fixed options, for a preview or a test that
/// wants to see the app with larger text, no motion or more contrast.
#[composable]
pub fn ProvideAccessibilityOptions(options: AccessibilityOptions, content: impl FnOnce()) {
    let provided = local_accessibility_options().provides(options.normalized());
    CompositionLocalProvider([provided], move || content());
}

#[cfg(test)]
#[path = "tests/accessibility_options_tests.rs"]
mod tests;
