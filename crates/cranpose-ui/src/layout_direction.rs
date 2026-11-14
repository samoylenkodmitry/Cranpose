//! Which way the interface reads.
//!
//! Layout direction is what turns "start" and "end" into "left" and "right".
//! It is a composition local rather than a global so a screen can pin a
//! direction — a code block, a phone number, a language picker — without
//! reversing the rest of the interface with it.

use cranpose_core::{CompositionLocalProvider, StaticCompositionLocal, staticCompositionLocalOf};
pub use cranpose_ui_layout::LayoutDirection;

/// The [`StaticCompositionLocal`] carrying the current layout direction.
///
/// Compose's `LocalLayoutDirection`, static as Compose's is: a read
/// subscribes nothing, and a provider given another direction recomposes
/// everything under it. Every layout reads it, and it seldom changes.
pub fn local_layout_direction() -> StaticCompositionLocal<LayoutDirection> {
    crate::thread_state::with_environment(|locals| {
        locals
            .direction
            .get_or_init(|| staticCompositionLocalOf(LayoutDirection::default))
            .clone()
    })
}

/// The layout direction in force here.
pub fn layout_direction() -> LayoutDirection {
    crate::thread_state::with_environment(|locals| {
        locals
            .direction
            .get_or_init(|| staticCompositionLocalOf(LayoutDirection::default))
            .current()
    })
}

/// Runs `content` in `direction`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn ProvideLayoutDirection(direction: LayoutDirection, content: impl FnOnce()) {
    CompositionLocalProvider([local_layout_direction().provides(direction)], content);
}

#[cfg(test)]
#[path = "tests/layout_direction_tests.rs"]
mod tests;
