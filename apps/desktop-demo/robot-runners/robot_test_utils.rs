//! Shared utility functions for robot tests.
//!
//! This module provides common helpers for semantics tree navigation
//! and debugging that are used across multiple robot tests.

use compose_app::SemanticElement;
use std::collections::HashMap;

/// Finds an element in the semantics tree by exact text match.
/// Searches recursively through all children.
#[allow(dead_code)]
pub fn find_element_by_text_exact<'a>(
    elements: &'a [SemanticElement],
    text: &str,
) -> Option<&'a SemanticElement> {
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            return Some(elem);
        }
        if let Some(found) = find_element_by_text_exact(&elem.children, text) {
            return Some(found);
        }
    }
    None
}

/// Finds an element within a subtree by exact text match.
/// Starts from a specific element and searches its descendants.
#[allow(dead_code)]
pub fn find_in_subtree_by_text<'a>(
    elem: &'a SemanticElement,
    text: &str,
) -> Option<&'a SemanticElement> {
    if elem.text.as_deref() == Some(text) {
        return Some(elem);
    }
    for child in &elem.children {
        if let Some(found) = find_in_subtree_by_text(child, text) {
            return Some(found);
        }
    }
    None
}

/// Prints the semantics tree with bounds information for debugging.
/// Each element is printed with its role, text, bounds, and clickable status.
#[allow(dead_code)]
pub fn print_semantics_with_bounds(elements: &[SemanticElement], indent: usize) {
    for elem in elements {
        let prefix = "  ".repeat(indent);
        let text = elem.text.as_deref().unwrap_or("");
        println!(
            "{}role={} text=\"{}\" bounds=({:.1},{:.1},{:.1},{:.1}){}",
            prefix,
            elem.role,
            text,
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
            if elem.clickable { " [CLICKABLE]" } else { "" }
        );
        print_semantics_with_bounds(&elem.children, indent + 1);
    }
}

/// Computes the union of two bounding boxes.
/// Returns the smallest bounding box that contains both inputs.
#[allow(dead_code)]
pub fn union_bounds(
    base: (f32, f32, f32, f32),
    other: Option<(f32, f32, f32, f32)>,
) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = base;
    let mut min_x = x;
    let mut min_y = y;
    let mut max_x = x + w;
    let mut max_y = y + h;
    if let Some((ox, oy, ow, oh)) = other {
        min_x = min_x.min(ox);
        min_y = min_y.min(oy);
        max_x = max_x.max(ox + ow);
        max_y = max_y.max(oy + oh);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Counts occurrences of exact text matches in the semantics tree.
#[allow(dead_code)]
pub fn count_text_in_tree(elements: &[SemanticElement], text: &str) -> usize {
    let mut count = 0;
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            count += 1;
        }
        count += count_text_in_tree(&elem.children, text);
    }
    count
}

/// Collects all elements matching exact text into a results vector.
#[allow(dead_code)]
pub fn collect_by_text_exact<'a>(
    elements: &'a [SemanticElement],
    text: &str,
    results: &mut Vec<&'a SemanticElement>,
) {
    for elem in elements {
        if elem.text.as_deref() == Some(text) {
            results.push(elem);
        }
        collect_by_text_exact(&elem.children, text, results);
    }
}

/// Collects counts of all text nodes matching a prefix.
#[allow(dead_code)]
pub fn collect_text_prefix_counts(
    elements: &[SemanticElement],
    prefix: &str,
    counts: &mut HashMap<String, usize>,
) {
    for elem in elements {
        if let Some(text) = elem.text.as_deref() {
            if text.starts_with(prefix) {
                *counts.entry(text.to_string()).or_insert(0) += 1;
            }
        }
        collect_text_prefix_counts(&elem.children, prefix, counts);
    }
}
