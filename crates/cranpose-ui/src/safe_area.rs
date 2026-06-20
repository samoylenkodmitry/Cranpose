//! Platform safe-area insets exposed to composition.

use std::cell::RefCell;

use cranpose_core::{compositionLocalOf, CompositionLocal};
use cranpose_ui_graphics::EdgeInsets;

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

#[cfg(test)]
mod tests {
    use super::local_safe_area_insets;
    use cranpose_ui_graphics::EdgeInsets;

    #[test]
    fn defaults_to_zero_insets() {
        assert_eq!(
            local_safe_area_insets().default_value(),
            EdgeInsets::default()
        );
    }

    #[test]
    fn returns_one_shared_local_per_thread() {
        // `CompositionLocal` is not `Debug`, so compare with `==` directly.
        assert!(local_safe_area_insets() == local_safe_area_insets());
    }
}
