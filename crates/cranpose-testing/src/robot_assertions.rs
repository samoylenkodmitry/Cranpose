//! Assertion utilities for robot testing
//!
//! This module provides assertion helpers specifically designed for
//! validating UI state in robot tests.

use cranpose_ui_graphics::Rect;

/// Assert that a value is within an expected range.
///
/// This is useful for fuzzy matching of positions and sizes that might
/// vary slightly due to rendering.
///
/// # Example
/// ```
/// use cranpose_testing::robot_assertions::assert_approx_eq;
/// assert_approx_eq(100.05, 100.0, 0.1, "Width should be approx 100");
/// ```
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
///
/// Checks x, y, width, and height individually with the given tolerance.
///
/// # Example
/// ```
/// use cranpose_testing::robot_assertions::assert_rect_approx_eq;
/// use cranpose_ui_graphics::Rect;
///
/// let r1 = Rect::new(0.0, 0.0, 100.0, 100.0);
/// let r2 = Rect::new(0.1, 0.0, 100.0, 99.9);
/// assert_rect_approx_eq(r1, r2, 0.2, "Rects should match");
/// ```
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
///
/// Useful for verifying that a click coordinate falls within an element.
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
///
/// Often used with `robot.get_all_text()` to verify content presence.
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
///
/// Verifies absence of content (e.g. after deletion or filtering).
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

// ============================================================================
// Semantic Tree Helpers
// ============================================================================

/// Bounds of a UI element (x, y, width, height)
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    /// Get the center point of these bounds
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// Generic semantic element trait for tree traversal
/// This allows the helpers to work with both cranpose::SemanticElement and similar types
pub trait SemanticElementLike {
    fn text(&self) -> Option<&str>;
    fn role(&self) -> &str;
    fn clickable(&self) -> bool;
    fn bounds(&self) -> Bounds;
    fn children(&self) -> &[Self]
    where
        Self: Sized;
}

/// Find an element by text content in the semantic tree.
/// Returns the center coordinates (x, y) if found.
///
/// # Type Parameters
/// * `E`: A type implementing `SemanticElementLike` (e.g. `SemanticElement`)
pub fn find_text_center<E: SemanticElementLike>(elements: &[E], text: &str) -> Option<(f32, f32)> {
    fn search<E: SemanticElementLike>(elem: &E, text: &str) -> Option<(f32, f32)> {
        if let Some(t) = elem.text() {
            if t.contains(text) {
                return Some(elem.bounds().center());
            }
        }
        for child in elem.children() {
            if let Some(pos) = search(child, text) {
                return Some(pos);
            }
        }
        None
    }

    for elem in elements {
        if let Some(pos) = search(elem, text) {
            return Some(pos);
        }
    }
    None
}

/// Find an element by text content and return full bounds.
pub fn find_text_bounds<E: SemanticElementLike>(elements: &[E], text: &str) -> Option<Bounds> {
    fn search<E: SemanticElementLike>(elem: &E, text: &str) -> Option<Bounds> {
        if let Some(t) = elem.text() {
            if t.contains(text) {
                return Some(elem.bounds());
            }
        }
        for child in elem.children() {
            if let Some(bounds) = search(child, text) {
                return Some(bounds);
            }
        }
        None
    }

    for elem in elements {
        if let Some(bounds) = search(elem, text) {
            return Some(bounds);
        }
    }
    None
}

/// Find a clickable element (button) containing the specified text.
/// Returns the bounds (x, y, width, height) if found.
pub fn find_button_bounds<E: SemanticElementLike>(elements: &[E], text: &str) -> Option<Bounds> {
    fn has_text<E: SemanticElementLike>(elem: &E, text: &str) -> bool {
        if let Some(t) = elem.text() {
            if t.contains(text) {
                return true;
            }
        }
        elem.children().iter().any(|c| has_text(c, text))
    }

    fn search<E: SemanticElementLike>(elem: &E, text: &str) -> Option<Bounds> {
        if elem.clickable() && has_text(elem, text) {
            return Some(elem.bounds());
        }
        for child in elem.children() {
            if let Some(bounds) = search(child, text) {
                return Some(bounds);
            }
        }
        None
    }

    for elem in elements {
        if let Some(bounds) = search(elem, text) {
            return Some(bounds);
        }
    }
    None
}

/// Find all elements matching a role (e.g., "Layout", "Text").
pub fn find_elements_by_role<E: SemanticElementLike>(elements: &[E], role: &str) -> Vec<Bounds> {
    fn search<E: SemanticElementLike>(elem: &E, role: &str, results: &mut Vec<Bounds>) {
        if elem.role() == role {
            results.push(elem.bounds());
        }
        for child in elem.children() {
            search(child, role, results);
        }
    }

    let mut results = Vec::new();
    for elem in elements {
        search(elem, role, &mut results);
    }
    results
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

    #[test]
    fn test_bounds_center() {
        let bounds = Bounds {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let (cx, cy) = bounds.center();
        assert_eq!(cx, 60.0);
        assert_eq!(cy, 45.0);
    }
}
