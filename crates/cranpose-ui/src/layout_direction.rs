//! Which way the interface reads.
//!
//! Layout direction is what turns "start" and "end" into "left" and "right".
//! It is a composition local rather than a global so a screen can pin a
//! direction — a code block, a phone number, a language picker — without
//! reversing the rest of the interface with it.

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};

/// Which side of the interface is the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayoutDirection {
    /// Start is the left edge — Latin, Cyrillic, CJK.
    #[default]
    Ltr,
    /// Start is the right edge — Arabic, Hebrew.
    Rtl,
}

impl LayoutDirection {
    /// Converts a distance from the start edge to a physical horizontal position.
    pub fn place_x(self, start: f32, width: f32, item_width: f32) -> f32 {
        if self.is_rtl() {
            width - start - item_width
        } else {
            start
        }
    }
    /// Whether the start edge is the right one.
    pub fn is_rtl(self) -> bool {
        matches!(self, LayoutDirection::Rtl)
    }

    /// The direction that reads the other way.
    pub fn reversed(self) -> Self {
        match self {
            LayoutDirection::Ltr => LayoutDirection::Rtl,
            LayoutDirection::Rtl => LayoutDirection::Ltr,
        }
    }

    /// Turns start/end values into physical left/right values.
    pub fn resolve(self, start: f32, end: f32) -> (f32, f32) {
        match self {
            LayoutDirection::Ltr => (start, end),
            LayoutDirection::Rtl => (end, start),
        }
    }
}

/// The [`CompositionLocal`] carrying the current layout direction.
pub fn local_layout_direction() -> CompositionLocal<LayoutDirection> {
    crate::environment_locals::ENVIRONMENT_LOCALS.with(|locals| {
        locals
            .direction
            .get_or_init(|| compositionLocalOf(LayoutDirection::default))
            .clone()
    })
}

/// The layout direction in force here.
pub fn layout_direction() -> LayoutDirection {
    crate::environment_locals::ENVIRONMENT_LOCALS.with(|locals| {
        locals
            .direction
            .get_or_init(|| compositionLocalOf(LayoutDirection::default))
            .current()
    })
}

/// Runs `content` in `direction`.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideLayoutDirection(direction: LayoutDirection, content: impl FnOnce()) {
    CompositionLocalProvider([local_layout_direction().provides(direction)], content);
}

#[cfg(test)]
#[path = "tests/layout_direction_tests.rs"]
mod tests;
