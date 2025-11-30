//! Assertion utilities for robot testing
//!
//! This module provides assertion helpers specifically designed for
//! validating UI state in robot tests.

use compose_ui_graphics::Rect;

/// Assert that a value is within an expected range.
///
/// This is useful for fuzzy matching of positions and sizes that might
/// vary slightly due to rendering.
pub fn assert_approx_eq(actual: f32, expected: f32, tolerance: f32, msg: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff <= tolerance,
        "{}: expected {} (±{}), got {} (diff: {})",
        msg,
        expected,
        tolerance,
        actual,
        diff
    );
}

/// Assert that a rectangle is approximately equal to another.
pub fn assert_rect_approx_eq(actual: Rect, expected: Rect, tolerance: f32, msg: &str) {
    assert_approx_eq(actual.x, expected.x, tolerance, &format!("{} - x", msg));
    assert_approx_eq(actual.y, expected.y, tolerance, &format!("{} - y", msg));
    assert_approx_eq(
        actual.width,
        expected.width,
        tolerance,
        &format!("{} - width", msg),
    );
    assert_approx_eq(
        actual.height,
        expected.height,
        tolerance,
        &format!("{} - height", msg),
    );
}

/// Assert that a rectangle contains a point.
pub fn assert_rect_contains_point(rect: Rect, x: f32, y: f32, msg: &str) {
    assert!(
        x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height,
        "{}: point ({}, {}) not in rect {:?}",
        msg,
        x,
        y,
        rect
    );
}

/// Assert that a list contains a specific text fragment.
pub fn assert_contains_text(texts: &[String], fragment: &str, msg: &str) {
    assert!(
        texts.iter().any(|t| t.contains(fragment)),
        "{}: text '{}' not found in {:?}",
        msg,
        fragment,
        texts
    );
}

/// Assert that a list does not contain a specific text fragment.
pub fn assert_not_contains_text(texts: &[String], fragment: &str, msg: &str) {
    assert!(
        !texts.iter().any(|t| t.contains(fragment)),
        "{}: text '{}' unexpectedly found in {:?}",
        msg,
        fragment,
        texts
    );
}

/// Assert that a collection has an expected count.
pub fn assert_count<T>(items: &[T], expected: usize, msg: &str) {
    assert_eq!(
        items.len(),
        expected,
        "{}: expected {} items, got {}",
        msg,
        expected,
        items.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approx_eq() {
        assert_approx_eq(100.0, 100.0, 0.1, "exact match");
        assert_approx_eq(100.05, 100.0, 0.1, "within tolerance");
    }

    #[test]
    #[should_panic]
    fn test_approx_eq_fails() {
        assert_approx_eq(100.5, 100.0, 0.1, "should fail");
    }

    #[test]
    fn test_rect_approx_eq() {
        let rect1 = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let rect2 = Rect {
            x: 10.05,
            y: 20.05,
            width: 100.05,
            height: 50.05,
        };
        assert_rect_approx_eq(rect1, rect2, 0.1, "nearly equal rects");
    }

    #[test]
    fn test_rect_contains_point() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        assert_rect_contains_point(rect, 50.0, 30.0, "center point");
        assert_rect_contains_point(rect, 10.0, 20.0, "top-left corner");
        assert_rect_contains_point(rect, 110.0, 70.0, "bottom-right corner");
    }

    #[test]
    fn test_contains_text() {
        let texts = vec!["Hello".to_string(), "World".to_string()];
        assert_contains_text(&texts, "Hello", "exact match");
        assert_contains_text(&texts, "Wor", "partial match");
        assert_not_contains_text(&texts, "Goodbye", "not present");
    }

    #[test]
    fn test_count() {
        let items = vec![1, 2, 3];
        assert_count(&items, 3, "correct count");
    }
}
