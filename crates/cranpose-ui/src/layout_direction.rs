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
    thread_local! {
        static LOCAL: std::cell::RefCell<Option<CompositionLocal<LayoutDirection>>> =
            const { std::cell::RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(LayoutDirection::default))
            .clone()
    })
}

/// The layout direction in force here.
pub fn layout_direction() -> LayoutDirection {
    local_layout_direction().current()
}

/// Runs `content` in `direction`.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideLayoutDirection(direction: LayoutDirection, content: impl FnOnce()) {
    CompositionLocalProvider(vec![local_layout_direction().provides(direction)], content);
}

#[cfg(test)]
#[path = "tests/layout_direction_tests.rs"]
mod tests;
