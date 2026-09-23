//! Whether a custom cursor has to be drawn smaller to keep the size it was
//! drawn at, given how much the system enlarges every cursor.

use crate::CustomCursorSize;

/// The system's cursor enlargement as a factor to divide by: 1 for a setting
/// that is missing, below 1 or not a number.
pub(crate) fn usable_scale(raw: f64) -> f64 {
    if raw.is_finite() && raw > 1.0 {
        raw
    } else {
        1.0
    }
}

/// Whether a custom cursor under `size` needs undoing the system's
/// enlargement `scale`.
pub(crate) fn compensates(size: CustomCursorSize, scale: f64) -> bool {
    size == CustomCursorSize::AsDrawn && usable_scale(scale) > 1.0
}

#[cfg(test)]
mod tests {
    use super::{compensates, usable_scale};
    use crate::CustomCursorSize;

    #[test]
    fn a_pointer_size_never_set_enlarges_nothing() {
        assert_eq!(usable_scale(0.0), 1.0, "an unset preference reads as 0");
        assert_eq!(usable_scale(0.5), 1.0);
        assert_eq!(usable_scale(f64::NAN), 1.0);
        assert_eq!(usable_scale(f64::INFINITY), 1.0);
        assert_eq!(usable_scale(2.5), 2.5);
    }

    #[test]
    fn only_a_cursor_asked_for_as_drawn_under_an_enlarged_pointer_is_redrawn() {
        assert!(compensates(CustomCursorSize::AsDrawn, 4.0));
        assert!(!compensates(CustomCursorSize::AsDrawn, 1.0));
        assert!(!compensates(CustomCursorSize::FollowSystem, 4.0));
        assert!(
            !compensates(CustomCursorSize::default(), 4.0),
            "following the system is the default"
        );
    }
}
