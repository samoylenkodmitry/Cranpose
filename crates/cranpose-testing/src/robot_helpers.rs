//! Common helper functions for robot tests
//!
//! These helpers work with `cranpose::SemanticElement` to find and interact
//! with UI elements during robot testing.

use crate::robot_assertions::{Bounds, SemanticElementLike};
use cranpose::SemanticElement;

// Implement SemanticElementLike for cranpose::SemanticElement
// This allows using the generic assertion helpers from robot_assertions
impl SemanticElementLike for SemanticElement {
    fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    fn role(&self) -> &str {
        &self.role
    }

    fn clickable(&self) -> bool {
        self.clickable
    }

    fn bounds(&self) -> Bounds {
        Bounds {
            x: self.bounds.x,
            y: self.bounds.y,
            width: self.bounds.width,
            height: self.bounds.height,
        }
    }

    fn children(&self) -> &[Self] {
        &self.children
    }
}

/// Find an element by text content (partial match), returning bounds (x, y, width, height).
///
/// Searches the entire subtree depth-first.
///
/// # Returns
/// Some((x, y, width, height)) if found, None otherwise.
pub fn find_text(elem: &SemanticElement, text: &str) -> Option<(f32, f32, f32, f32)> {
    if let Some(ref t) = elem.text {
        if t.contains(text) {
            return Some((
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
            ));
        }
    }
    for child in &elem.children {
        if let Some(pos) = find_text(child, text) {
            return Some(pos);
        }
    }
    None
}

/// Find an element by exact text content, returning bounds (x, y, width, height).
///
/// Useful when partial matches might be ambiguous.
pub fn find_text_exact(elem: &SemanticElement, text: &str) -> Option<(f32, f32, f32, f32)> {
    if let Some(ref t) = elem.text {
        if t == text {
            return Some((
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
            ));
        }
    }
    for child in &elem.children {
        if let Some(pos) = find_text_exact(child, text) {
            return Some(pos);
        }
    }
    None
}

/// Find an element by text content, returning center coordinates (x, y).
///
/// This is a convenience helper for clicking elements.
pub fn find_text_center(elem: &SemanticElement, text: &str) -> Option<(f32, f32)> {
    find_text(elem, text).map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
}

/// Check if an element or any of its children contains the specified text.
pub fn has_text(elem: &SemanticElement, text: &str) -> bool {
    if let Some(ref t) = elem.text {
        if t.contains(text) {
            return true;
        }
    }
    elem.children.iter().any(|c| has_text(c, text))
}

/// Find a clickable element (button) containing the specified text.
///
/// Matches elements where `clickable == true` AND the element (or its children) contains the text.
/// Returns bounds (x, y, width, height).
pub fn find_button(elem: &SemanticElement, text: &str) -> Option<(f32, f32, f32, f32)> {
    if elem.clickable && has_text(elem, text) {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
    }
    for child in &elem.children {
        if let Some(pos) = find_button(child, text) {
            return Some(pos);
        }
    }
    None
}

/// Find a clickable element (button) containing the specified text.
/// Returns center coordinates (x, y).
pub fn find_button_center(elem: &SemanticElement, text: &str) -> Option<(f32, f32)> {
    find_button(elem, text).map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
}

/// Search the semantic tree from Robot, applying a finder function.
///
/// This handles the boilerplate of fetching semantics and iterating through roots.
/// It creates a unified search context for finder functions.
///
/// # Returns
/// The first result (x, y, w, h) returned by the finder function.
pub fn find_in_semantics<F>(robot: &cranpose::Robot, finder: F) -> Option<(f32, f32, f32, f32)>
where
    F: Fn(&SemanticElement) -> Option<(f32, f32, f32, f32)>,
{
    match robot.get_semantics() {
        Ok(semantics) => {
            for root in semantics.iter() {
                if let Some(result) = finder(root) {
                    return Some(result);
                }
            }
            None
        }
        Err(e) => {
            eprintln!("  ✗ Failed to get semantics: {}", e);
            None
        }
    }
}

/// Find element by text in semantics tree (Robot wrapper).
///
/// Fetches the current semantics tree from the robot and searches for text.
/// Returns bounds (x, y, width, height).
pub fn find_text_in_semantics(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    let text = text.to_string();
    find_in_semantics(robot, |elem| find_text(elem, &text))
}

/// Find an element whose text starts with the given prefix.
/// Returns bounds (x, y, width, height) and the full text.
pub fn find_text_by_prefix(
    elem: &SemanticElement,
    prefix: &str,
) -> Option<(f32, f32, f32, f32, String)> {
    if let Some(ref t) = elem.text {
        if t.starts_with(prefix) {
            return Some((
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
                t.clone(),
            ));
        }
    }
    for child in &elem.children {
        if let Some(result) = find_text_by_prefix(child, prefix) {
            return Some(result);
        }
    }
    None
}

/// Find element by text prefix in semantics tree.
/// Returns bounds (x, y, width, height) and the full text content.
/// Useful for parsing dynamic text like "Stats: C=5 E=3 D=2".
pub fn find_text_by_prefix_in_semantics(
    robot: &cranpose::Robot,
    prefix: &str,
) -> Option<(f32, f32, f32, f32, String)> {
    let prefix = prefix.to_string();
    match robot.get_semantics() {
        Ok(semantics) => {
            for root in semantics.iter() {
                if let Some(result) = find_text_by_prefix(root, &prefix) {
                    return Some(result);
                }
            }
            None
        }
        Err(e) => {
            eprintln!("  ✗ Failed to get semantics: {}", e);
            None
        }
    }
}

/// Find button by text in semantics tree.
/// Convenience wrapper around find_in_semantics + find_button.
pub fn find_button_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    let text = text.to_string();
    find_in_semantics(robot, |elem| find_button(elem, &text))
}

/// Recursively search for text in semantic elements.
/// Returns the element containing the text.
pub fn find_by_text_recursive(elements: &[SemanticElement], text: &str) -> Option<SemanticElement> {
    for elem in elements {
        if let Some(ref elem_text) = elem.text {
            if elem_text.contains(text) {
                return Some(elem.clone());
            }
        }
        if let Some(found) = find_by_text_recursive(&elem.children, text) {
            return Some(found);
        }
    }
    None
}

/// Find all clickable elements in a specific Y range.
/// Returns a list of (label, x, y) tuples sorted by x position.
pub fn find_clickables_in_range(
    elements: &[SemanticElement],
    min_y: f32,
    max_y: f32,
) -> Vec<(String, f32, f32)> {
    fn search(elem: &SemanticElement, tabs: &mut Vec<(String, f32, f32)>, min_y: f32, max_y: f32) {
        if elem.role == "Layout" && elem.clickable && elem.bounds.y > min_y && elem.bounds.y < max_y
        {
            let label = elem
                .children
                .iter()
                .find(|child| child.role == "Text")
                .and_then(|text_elem| text_elem.text.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            tabs.push((label, elem.bounds.x, elem.bounds.y));
        }
        for child in &elem.children {
            search(child, tabs, min_y, max_y);
        }
    }

    let mut tabs = Vec::new();
    for elem in elements {
        search(elem, &mut tabs, min_y, max_y);
    }
    tabs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    tabs
}
