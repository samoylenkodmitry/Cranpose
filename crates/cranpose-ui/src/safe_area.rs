//! Platform safe-area insets exposed to composition.

use std::cell::RefCell;

use cranpose_core::{CompositionLocal, compositionLocalOf};
use cranpose_ui_graphics::EdgeInsets;

use crate::Modifier;

/// Insets needed to keep content clear of system UI and the on-screen keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowInsets {
    pub safe_area: EdgeInsets,
    pub ime: EdgeInsets,
}

impl WindowInsets {
    /// Combines overlapping insets by taking the largest obstruction on each edge.
    pub fn combined(self) -> EdgeInsets {
        EdgeInsets::from_components(
            self.safe_area.left.max(self.ime.left),
            self.safe_area.top.max(self.ime.top),
            self.safe_area.right.max(self.ime.right),
            self.safe_area.bottom.max(self.ime.bottom),
        )
    }
}

/// CompositionLocal carrying the platform safe-area insets in logical pixels.
///
/// Mobile backends provide the system-bar / notch / home-indicator insets here
/// so applications can keep content clear of them (for example with
/// `Modifier.padding_each`). It defaults to [`EdgeInsets::default`] (zero), so
/// desktop and web compositions are unaffected.
///
/// The same `CompositionLocal` instance is returned on every call (cached per
/// thread), so the platform that provides it and the app that reads it observe
/// one shared local.
pub fn local_safe_area_insets() -> CompositionLocal<EdgeInsets> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<EdgeInsets>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(EdgeInsets::default))
            .clone()
    })
}

/// CompositionLocal carrying the on-screen soft-keyboard (IME) insets in logical
/// pixels. The `bottom` field is the height the keyboard currently covers at the
/// bottom of the window (zero when it is hidden); the other edges are zero.
///
/// Mobile backends update this as the keyboard animates in and out so an
/// application can keep the focused field visible — for example by adding
/// `bottom` to a scroll container's bottom padding, or scrolling the caret into
/// view. It defaults to [`EdgeInsets::default`] (zero), so desktop and web
/// compositions (and mobile frames with no keyboard) are unaffected.
///
/// Like [`local_safe_area_insets`], the same `CompositionLocal` instance is
/// returned on every call (cached per thread).
pub fn local_ime_insets() -> CompositionLocal<EdgeInsets> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<EdgeInsets>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(EdgeInsets::default))
            .clone()
    })
}

/// Returns the framework-owned insets currently visible to the composition.
pub fn window_insets() -> WindowInsets {
    WindowInsets {
        safe_area: local_safe_area_insets().current(),
        ime: local_ime_insets().current(),
    }
}

impl Modifier {
    /// Adds padding for explicit platform or application insets.
    pub fn window_insets_padding(self, insets: EdgeInsets) -> Self {
        self.padding_each(insets.left, insets.top, insets.right, insets.bottom)
    }

    /// Adds padding for system bars, display cutouts, and rounded display edges.
    pub fn safe_area_padding(self) -> Self {
        self.window_insets_padding(local_safe_area_insets().current())
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::EdgeInsets;

    use super::{WindowInsets, local_ime_insets, local_safe_area_insets};

    #[test]
    fn defaults_to_zero_insets() {
        assert_eq!(
            local_safe_area_insets().default_value(),
            EdgeInsets::default()
        );
    }

    #[test]
    fn ime_insets_default_to_zero() {
        assert_eq!(local_ime_insets().default_value(), EdgeInsets::default());
    }

    #[test]
    fn returns_one_shared_local_per_thread() {
        // `CompositionLocal` is not `Debug`, so compare with `==` directly.
        assert!(local_safe_area_insets() == local_safe_area_insets());
        assert!(local_ime_insets() == local_ime_insets());
        // The IME local is distinct from the safe-area local.
        assert!(local_ime_insets() != local_safe_area_insets());
    }

    #[test]
    fn combined_insets_do_not_double_count_overlapping_edges() {
        let combined = WindowInsets {
            safe_area: EdgeInsets::from_components(2.0, 8.0, 4.0, 20.0),
            ime: EdgeInsets::from_components(0.0, 0.0, 6.0, 100.0),
        }
        .combined();
        assert_eq!(combined, EdgeInsets::from_components(2.0, 8.0, 6.0, 100.0));
    }

    #[test]
    fn explicit_window_insets_become_padding() {
        use crate::{Modifier, modifier::ModifierChainHandle};

        let _app_context = crate::render_state::app_context_test_scope();
        let insets = EdgeInsets::from_components(1.0, 2.0, 3.0, 4.0);
        let mut handle = ModifierChainHandle::new();
        let _ = handle.update(&Modifier::empty().window_insets_padding(insets));
        assert_eq!(handle.resolved_modifiers().padding(), insets);
    }
}
