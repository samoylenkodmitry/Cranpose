//! Common helper functions for robot tests
//!
//! These helpers work with `cranpose::SemanticElement` to find and interact
//! with UI elements during robot testing.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cranpose::SemanticElement;

use crate::robot_assertions::{Bounds, SemanticElementLike};

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
    if let Some(ref t) = elem.text
        && t.contains(text)
    {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
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
    if let Some(ref t) = elem.text
        && t == text
    {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
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
    has_text_by(elem, text, text_contains)
}

/// Find a clickable element (button) containing the specified text.
///
/// Matches elements where `clickable == true` AND the element (or its children) contains the text.
/// Returns bounds (x, y, width, height).
pub fn find_button(elem: &SemanticElement, text: &str) -> Option<(f32, f32, f32, f32)> {
    find_button_by(elem, text, text_contains)
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
    match robot.find_text_bounds(text) {
        Ok(bounds) => bounds,
        Err(e) => {
            eprintln!("  ✗ Failed to query text semantics: {}", e);
            None
        }
    }
}

/// Find an element whose text starts with the given prefix.
/// Returns bounds (x, y, width, height) and the full text.
pub fn find_text_by_prefix(
    elem: &SemanticElement,
    prefix: &str,
) -> Option<(f32, f32, f32, f32, String)> {
    if let Some(ref t) = elem.text
        && t.starts_with(prefix)
    {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
            t.clone(),
        ));
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
    match robot.find_text_by_prefix(prefix) {
        Ok(bounds) => bounds,
        Err(e) => {
            eprintln!("  ✗ Failed to query text prefix semantics: {}", e);
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
    find_button_in_semantics_by(robot, text, TextMatchMode::Contains)
}

/// Find button by exact text in semantics tree.
pub fn find_button_exact_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    find_button_in_semantics_by(robot, text, TextMatchMode::Exact)
}

fn find_button_in_semantics_by(
    robot: &cranpose::Robot,
    text: &str,
    match_mode: TextMatchMode,
) -> Option<(f32, f32, f32, f32)> {
    let text_owned = text.to_string();

    button_helper_diag(&text_owned, "resolve root bounds");
    let Some(root) = root_bounds(robot) else {
        button_helper_diag(&text_owned, "root unavailable; using first raw match");
        let result = find_button_bounds_for_mode(robot, &text_owned, match_mode)
            .into_iter()
            .next();
        button_helper_diag(&text_owned, &format!("raw result={result:?}"));
        return result;
    };
    button_helper_diag(&text_owned, &format!("root={root:?}"));
    let mut candidates = find_button_bounds_for_mode(robot, &text_owned, match_mode);
    button_helper_diag(&text_owned, &format!("candidates={candidates:?}"));
    let mut bounds = choose_button_bounds(&candidates, root);
    button_helper_diag(&text_owned, &format!("chosen={bounds:?}"));

    if let Some(current) = visible_bounds(bounds, root) {
        button_helper_diag(&text_owned, &format!("visible={current:?}"));
        return Some(current);
    }
    if bounds.is_none() {
        button_helper_diag(&text_owned, "no candidate");
        return None;
    }

    for attempt in 0..8 {
        let Some(current) = bounds else {
            break;
        };
        button_helper_diag(
            &text_owned,
            &format!("scroll attempt={attempt} current={current:?}"),
        );

        let Some((axis, _dir)) = overflow_axis_direction(current, root) else {
            button_helper_diag(&text_owned, "no overflow axis");
            break;
        };

        let Some((scroll_delta_x, scroll_delta_y)) = scroll_delta_for_overflow(current, root, axis)
        else {
            button_helper_diag(&text_owned, "no scroll delta");
            break;
        };
        button_helper_diag(
            &text_owned,
            &format!("axis={axis:?} delta=({scroll_delta_x:.1},{scroll_delta_y:.1})"),
        );

        button_helper_diag(&text_owned, "query semantics for scroll anchor");
        let semantics = match robot.get_semantics() {
            Ok(semantics) => semantics,
            Err(e) => {
                eprintln!("  ✗ Failed to get semantics: {}", e);
                break;
            }
        };

        let Some((sx, sy, sw, sh)) = find_scroll_anchor(&semantics, current, root, axis) else {
            button_helper_diag(&text_owned, "no scroll anchor");
            break;
        };

        let start_x = sx + sw / 2.0;
        let start_y = sy + sh / 2.0;
        button_helper_diag(
            &text_owned,
            &format!("mouse_move=({start_x:.1},{start_y:.1})"),
        );
        let _ = robot.mouse_move(start_x, start_y);
        button_helper_diag(&text_owned, "mouse_scroll begin");
        let _ = robot.mouse_scroll(scroll_delta_x, scroll_delta_y);
        button_helper_diag(&text_owned, "mouse_scroll end");
        std::thread::sleep(Duration::from_millis(SCROLL_SETTLE_MS));
        button_helper_diag(&text_owned, "pump begin");
        let _ = robot.pump_frames(1);
        button_helper_diag(&text_owned, "pump end");

        candidates = find_button_bounds_for_mode(robot, &text_owned, match_mode);
        button_helper_diag(&text_owned, &format!("next candidates={candidates:?}"));
        bounds = choose_button_bounds(&candidates, root);
        button_helper_diag(&text_owned, &format!("next chosen={bounds:?}"));
        if let Some(current) = visible_bounds(bounds, root) {
            button_helper_diag(&text_owned, &format!("visible after scroll={current:?}"));
            return Some(current);
        }
        if bounds.is_some_and(|next| rect_bounds_close(next, current)) {
            button_helper_diag(&text_owned, "scroll made no candidate progress");
            break;
        }
    }

    let result = visible_bounds(bounds, root);
    button_helper_diag(&text_owned, &format!("final={result:?}"));
    result
}

fn button_helper_diag(text: &str, message: &str) {
    if std::env::var_os("CRANPOSE_ROBOT_BUTTON_HELPER_DIAG").is_some() {
        eprintln!("button-helper {text:?}: {message}");
    }
}

fn rect_bounds_close(left: RectBounds, right: RectBounds) -> bool {
    const EPSILON: f32 = 0.5;
    (left.0 - right.0).abs() <= EPSILON
        && (left.1 - right.1).abs() <= EPSILON
        && (left.2 - right.2).abs() <= EPSILON
        && (left.3 - right.3).abs() <= EPSILON
}

/// Recursively search for text in semantic elements.
/// Returns the element containing the text.
pub fn find_by_text_recursive(elements: &[SemanticElement], text: &str) -> Option<SemanticElement> {
    for elem in elements {
        if let Some(ref elem_text) = elem.text
            && elem_text.contains(text)
        {
            return Some(elem.clone());
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
    tabs.sort_by(|a, b| a.1.total_cmp(&b.1));
    tabs
}

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

pub fn find_bounds_by_text(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32, f32, f32)> {
    let semantics = robot.get_semantics().ok()?;
    let elem = find_element_by_text_exact(&semantics, text)?;
    Some((
        elem.bounds.x,
        elem.bounds.y,
        elem.bounds.width,
        elem.bounds.height,
    ))
}

pub fn visible_bounds_in_viewport(
    robot: &cranpose::Robot,
    bounds: (f32, f32, f32, f32),
    padding: f32,
) -> Option<(f32, f32, f32, f32)> {
    let semantics = robot.get_semantics().ok()?;
    let mut viewport = None;
    for elem in semantics.iter() {
        let elem_bounds = (
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        );
        viewport = Some(match viewport {
            Some(existing) => union_bounds(existing, Some(elem_bounds)),
            None => elem_bounds,
        });
    }
    let (viewport_x, viewport_y, viewport_width, viewport_height) = viewport?;
    let min_x = viewport_x + padding;
    let min_y = viewport_y + padding;
    let max_x = viewport_x + viewport_width - padding;
    let max_y = viewport_y + viewport_height - padding;

    let left = bounds.0.max(min_x);
    let top = bounds.1.max(min_y);
    let right = (bounds.0 + bounds.2).min(max_x);
    let bottom = (bounds.1 + bounds.3).min(max_y);

    if right <= left || bottom <= top {
        None
    } else {
        Some((left, top, right - left, bottom - top))
    }
}

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

pub fn collect_text_prefix_counts(
    elements: &[SemanticElement],
    prefix: &str,
    counts: &mut HashMap<String, usize>,
) {
    for elem in elements {
        if let Some(text) = elem.text.as_deref()
            && text.starts_with(prefix)
        {
            *counts.entry(text.to_string()).or_insert(0) += 1;
        }
        collect_text_prefix_counts(&elem.children, prefix, counts);
    }
}

pub fn exit_with_timeout(robot: &cranpose::Robot, timeout: Duration) {
    let done = Arc::new(AtomicBool::new(false));
    let done_thread = Arc::clone(&done);
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        if !done_thread.load(Ordering::Relaxed) {
            std::process::exit(0);
        }
    });

    let _ = robot.exit();
    done.store(true, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAxis {
    Horizontal,
    Vertical,
}

type RectBounds = (f32, f32, f32, f32);
type LabeledRect = (String, RectBounds);

pub fn collect_tab_bounds(robot: &cranpose::Robot, labels: &[&str]) -> Vec<LabeledRect> {
    let mut tabs = Vec::new();
    for label in labels {
        if let Some(bounds) =
            find_in_semantics(robot, |elem| find_button_by(elem, label, text_equals))
        {
            tabs.push(((*label).to_string(), bounds));
        }
    }
    tabs
}

pub fn bounds_span(bounds: &[LabeledRect]) -> Option<RectBounds> {
    let mut iter = bounds.iter();
    let (_, (x, y, w, h)) = iter.next()?;
    let mut min_x = *x;
    let mut min_y = *y;
    let mut max_x = x + w;
    let mut max_y = y + h;
    for (_, (bx, by, bw, bh)) in iter {
        min_x = min_x.min(*bx);
        min_y = min_y.min(*by);
        max_x = max_x.max(*bx + *bw);
        max_y = max_y.max(*by + *bh);
    }
    Some((min_x, min_y, max_x, max_y))
}

pub fn detect_tab_axis(bounds: &[LabeledRect]) -> Option<TabAxis> {
    let (min_x, min_y, max_x, max_y) = bounds_span(bounds)?;
    let span_x = max_x - min_x;
    let span_y = max_y - min_y;
    if span_x >= span_y {
        Some(TabAxis::Horizontal)
    } else {
        Some(TabAxis::Vertical)
    }
}

pub fn root_bounds(robot: &cranpose::Robot) -> Option<RectBounds> {
    if let Ok(screenshot) = robot.screenshot()
        && screenshot.logical_width.is_finite()
        && screenshot.logical_width > 0.0
        && screenshot.logical_height.is_finite()
        && screenshot.logical_height > 0.0
    {
        return Some((
            0.0,
            0.0,
            screenshot.logical_width,
            screenshot.logical_height,
        ));
    }

    let semantics = robot.get_semantics().ok()?;
    let root = semantics.first()?;
    Some((
        root.bounds.x,
        root.bounds.y,
        root.bounds.width,
        root.bounds.height,
    ))
}

const VISIBILITY_PADDING: f32 = 4.0;
const VISIBILITY_EDGE_EPSILON: f32 = 1.0;
const SCROLL_ANCHOR_EDGE_PADDING: f32 = 72.0;
const SCROLL_SETTLE_MS: u64 = 140;
type TextMatcher = fn(&str, &str) -> bool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextMatchMode {
    Contains,
    Exact,
}

fn text_contains(actual: &str, needle: &str) -> bool {
    actual.contains(needle)
}

fn text_equals(actual: &str, needle: &str) -> bool {
    actual == needle
}

fn find_button_bounds_for_mode(
    robot: &cranpose::Robot,
    text: &str,
    match_mode: TextMatchMode,
) -> Vec<RectBounds> {
    let semantics = match robot.get_semantics() {
        Ok(semantics) => semantics,
        Err(e) => {
            eprintln!("  ✗ Failed to query button semantics: {}", e);
            return Vec::new();
        }
    };
    let matcher = match match_mode {
        TextMatchMode::Contains => text_contains,
        TextMatchMode::Exact => text_equals,
    };
    let mut matches = Vec::new();
    collect_button_bounds_by(&semantics, text, matcher, &mut matches);
    matches
}

fn has_text_by(elem: &SemanticElement, text: &str, matcher: TextMatcher) -> bool {
    if elem
        .text
        .as_deref()
        .is_some_and(|actual| matcher(actual, text))
    {
        return true;
    }
    elem.children
        .iter()
        .any(|child| has_text_by(child, text, matcher))
}

fn find_button_by(
    elem: &SemanticElement,
    text: &str,
    matcher: TextMatcher,
) -> Option<(f32, f32, f32, f32)> {
    if elem.clickable && has_text_by(elem, text, matcher) {
        return Some((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
    }
    elem.children
        .iter()
        .find_map(|child| find_button_by(child, text, matcher))
}

fn collect_button_bounds_by(
    elements: &[SemanticElement],
    text: &str,
    matcher: TextMatcher,
    matches: &mut Vec<RectBounds>,
) {
    for elem in elements {
        if elem.clickable && has_text_by(elem, text, matcher) {
            matches.push((
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
            ));
        }
        collect_button_bounds_by(&elem.children, text, matcher, matches);
    }
}

fn choose_button_bounds(candidates: &[RectBounds], root: RectBounds) -> Option<RectBounds> {
    candidates
        .iter()
        .copied()
        .find(|bounds| is_fully_visible(*bounds, root))
        .or_else(|| {
            candidates.iter().copied().min_by(|left, right| {
                button_visibility_score(*left, root)
                    .total_cmp(&button_visibility_score(*right, root))
            })
        })
}

fn button_visibility_score(bounds: RectBounds, root: RectBounds) -> f32 {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    let left = rx;
    let right = rx + rw;
    let top = ry;
    let bottom = ry + rh;

    (left - x).max(0.0) + (x + w - right).max(0.0) + (top - y).max(0.0) + (y + h - bottom).max(0.0)
}

fn is_axis_visible(bounds: RectBounds, root: RectBounds, axis: TabAxis) -> bool {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    match axis {
        TabAxis::Horizontal => {
            x >= rx - VISIBILITY_EDGE_EPSILON && x + w <= rx + rw + VISIBILITY_EDGE_EPSILON
        }
        TabAxis::Vertical => {
            y >= ry - VISIBILITY_EDGE_EPSILON && y + h <= ry + rh + VISIBILITY_EDGE_EPSILON
        }
    }
}

fn is_fully_visible(bounds: RectBounds, root: RectBounds) -> bool {
    is_axis_visible(bounds, root, TabAxis::Horizontal)
        && is_axis_visible(bounds, root, TabAxis::Vertical)
}

fn visible_bounds(bounds: Option<RectBounds>, root: RectBounds) -> Option<RectBounds> {
    bounds.filter(|bounds| is_fully_visible(*bounds, root))
}

fn overflow_axis_direction(bounds: RectBounds, root: RectBounds) -> Option<(TabAxis, f32)> {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;

    let overflow_left = (rx - x).max(0.0);
    let overflow_right = (x + w - (rx + rw)).max(0.0);
    let overflow_top = (ry - y).max(0.0);
    let overflow_bottom = (y + h - (ry + rh)).max(0.0);

    let horizontal_overflow = overflow_left.max(overflow_right);
    let vertical_overflow = overflow_top.max(overflow_bottom);

    if horizontal_overflow <= 0.0 && vertical_overflow <= 0.0 {
        return None;
    }

    if horizontal_overflow >= vertical_overflow {
        if overflow_left > 0.0 {
            Some((TabAxis::Horizontal, 1.0))
        } else {
            Some((TabAxis::Horizontal, -1.0))
        }
    } else if overflow_top > 0.0 {
        Some((TabAxis::Vertical, 1.0))
    } else {
        Some((TabAxis::Vertical, -1.0))
    }
}

fn scroll_delta_for_overflow(
    bounds: RectBounds,
    root: RectBounds,
    axis: TabAxis,
) -> Option<(f32, f32)> {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    let left = rx + VISIBILITY_PADDING;
    let right = rx + rw - VISIBILITY_PADDING;
    let top = ry + VISIBILITY_PADDING;
    let bottom = ry + rh - VISIBILITY_PADDING;

    match axis {
        TabAxis::Horizontal if x < left - VISIBILITY_EDGE_EPSILON => Some((left - x, 0.0)),
        TabAxis::Horizontal if x + w > right + VISIBILITY_EDGE_EPSILON => {
            Some((-(x + w - right), 0.0))
        }
        TabAxis::Vertical if y < top - VISIBILITY_EDGE_EPSILON => Some((0.0, top - y)),
        TabAxis::Vertical if y + h > bottom + VISIBILITY_EDGE_EPSILON => {
            Some((0.0, -(y + h - bottom)))
        }
        _ => None,
    }
}

fn intersects_root(bounds: RectBounds, root: RectBounds) -> bool {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    let left = x.max(rx);
    let top = y.max(ry);
    let right = (x + w).min(rx + rw);
    let bottom = (y + h).min(ry + rh);
    right > left && bottom > top
}

fn overlap_len(a_start: f32, a_len: f32, b_start: f32, b_len: f32) -> f32 {
    let a_end = a_start + a_len;
    let b_end = b_start + b_len;
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

fn cross_axis_overlap(bounds: RectBounds, target: RectBounds, axis: TabAxis) -> f32 {
    match axis {
        TabAxis::Horizontal => overlap_len(bounds.1, bounds.3, target.1, target.3),
        TabAxis::Vertical => overlap_len(bounds.0, bounds.2, target.0, target.2),
    }
}

fn primary_axis_distance(bounds: RectBounds, target: RectBounds, axis: TabAxis) -> f32 {
    let center = match axis {
        TabAxis::Horizontal => bounds.0 + bounds.2 / 2.0,
        TabAxis::Vertical => bounds.1 + bounds.3 / 2.0,
    };
    let target_center = match axis {
        TabAxis::Horizontal => target.0 + target.2 / 2.0,
        TabAxis::Vertical => target.1 + target.3 / 2.0,
    };
    (center - target_center).abs()
}

fn scroll_anchor_is_edge_safe(bounds: RectBounds, root: RectBounds, axis: TabAxis) -> bool {
    let (rx, ry, rw, rh) = root;
    match axis {
        TabAxis::Horizontal => {
            let center_x = bounds.0 + bounds.2 / 2.0;
            center_x >= rx + SCROLL_ANCHOR_EDGE_PADDING
                && center_x <= rx + rw - SCROLL_ANCHOR_EDGE_PADDING
        }
        TabAxis::Vertical => {
            let center_y = bounds.1 + bounds.3 / 2.0;
            center_y >= ry + SCROLL_ANCHOR_EDGE_PADDING
                && center_y <= ry + rh - SCROLL_ANCHOR_EDGE_PADDING
        }
    }
}

fn find_scroll_anchor(
    elements: &[SemanticElement],
    target: RectBounds,
    root: RectBounds,
    axis: TabAxis,
) -> Option<RectBounds> {
    let mut best: Option<(RectBounds, bool, f32, f32)> = None;

    for elem in elements {
        if elem.clickable {
            let bounds = (
                elem.bounds.x,
                elem.bounds.y,
                elem.bounds.width,
                elem.bounds.height,
            );
            if is_fully_visible(bounds, root) && intersects_root(bounds, root) {
                let overlap = cross_axis_overlap(bounds, target, axis);
                if overlap > 0.0 {
                    let distance = primary_axis_distance(bounds, target, axis);
                    let edge_safe = scroll_anchor_is_edge_safe(bounds, root, axis);
                    match best {
                        None => best = Some((bounds, edge_safe, overlap, distance)),
                        Some((_, best_edge_safe, best_overlap, best_distance)) => {
                            let is_better_safety = edge_safe && !best_edge_safe;
                            let is_better_overlap = overlap > best_overlap + f32::EPSILON;
                            let is_better_distance = (overlap - best_overlap).abs() <= f32::EPSILON
                                && distance < best_distance;
                            if is_better_safety
                                || (edge_safe == best_edge_safe
                                    && (is_better_overlap || is_better_distance))
                            {
                                best = Some((bounds, edge_safe, overlap, distance));
                            }
                        }
                    }
                }
            }
        }

        if let Some(found) = find_scroll_anchor(&elem.children, target, root, axis) {
            let overlap = cross_axis_overlap(found, target, axis);
            let distance = primary_axis_distance(found, target, axis);
            let edge_safe = scroll_anchor_is_edge_safe(found, root, axis);
            match best {
                None => best = Some((found, edge_safe, overlap, distance)),
                Some((_, best_edge_safe, best_overlap, best_distance)) => {
                    let is_better_safety = edge_safe && !best_edge_safe;
                    let is_better_overlap = overlap > best_overlap + f32::EPSILON;
                    let is_better_distance =
                        (overlap - best_overlap).abs() <= f32::EPSILON && distance < best_distance;
                    if is_better_safety
                        || (edge_safe == best_edge_safe
                            && (is_better_overlap || is_better_distance))
                    {
                        best = Some((found, edge_safe, overlap, distance));
                    }
                }
            }
        }
    }

    best.map(|(bounds, _, _, _)| bounds)
}

/// Capture a full screenshot from the running robot app.
pub fn capture_screenshot(robot: &cranpose::Robot) -> Option<cranpose::RobotScreenshot> {
    robot.screenshot().ok()
}

fn screenshot_logical_width(screenshot: &cranpose::RobotScreenshot) -> f32 {
    if screenshot.logical_width.is_finite() && screenshot.logical_width > 0.0 {
        screenshot.logical_width
    } else {
        screenshot.width.max(1) as f32
    }
}

fn screenshot_logical_height(screenshot: &cranpose::RobotScreenshot) -> f32 {
    if screenshot.logical_height.is_finite() && screenshot.logical_height > 0.0 {
        screenshot.logical_height
    } else {
        screenshot.height.max(1) as f32
    }
}

fn screenshot_scale_x(screenshot: &cranpose::RobotScreenshot) -> f32 {
    screenshot.width.max(1) as f32 / screenshot_logical_width(screenshot)
}

fn screenshot_scale_y(screenshot: &cranpose::RobotScreenshot) -> f32 {
    screenshot.height.max(1) as f32 / screenshot_logical_height(screenshot)
}

fn logical_to_screenshot_x(screenshot: &cranpose::RobotScreenshot, x: f32) -> f32 {
    x * screenshot_scale_x(screenshot)
}

fn logical_to_screenshot_y(screenshot: &cranpose::RobotScreenshot, y: f32) -> f32 {
    y * screenshot_scale_y(screenshot)
}

pub fn screenshot_logical_size(screenshot: &cranpose::RobotScreenshot) -> (f32, f32) {
    (
        screenshot_logical_width(screenshot),
        screenshot_logical_height(screenshot),
    )
}

/// Returns pixel RGBA at `(x, y)` from a screenshot.
pub fn screenshot_pixel(screenshot: &cranpose::RobotScreenshot, x: u32, y: u32) -> Option<[u8; 4]> {
    if x >= screenshot.width || y >= screenshot.height {
        return None;
    }
    let index = screenshot_pixel_offset(screenshot, x, y)?;
    let pixel = screenshot.pixels.get(index..index + 4)?;
    Some([pixel[0], pixel[1], pixel[2], pixel[3]])
}

fn screenshot_pixel_offset(
    screenshot: &cranpose::RobotScreenshot,
    x: u32,
    y: u32,
) -> Option<usize> {
    let width = usize::try_from(screenshot.width).ok()?;
    let x = usize::try_from(x).ok()?;
    let y = usize::try_from(y).ok()?;
    y.checked_mul(width)?.checked_add(x)?.checked_mul(4)
}

fn expected_screenshot_pixel_bytes(screenshot: &cranpose::RobotScreenshot) -> Option<usize> {
    let width = usize::try_from(screenshot.width).ok()?;
    let height = usize::try_from(screenshot.height).ok()?;
    width.checked_mul(height)?.checked_mul(4)
}

fn screenshot_pixel_storage_complete(screenshot: &cranpose::RobotScreenshot) -> bool {
    expected_screenshot_pixel_bytes(screenshot)
        .is_some_and(|expected| screenshot.pixels.len() >= expected)
}

pub fn sample_screenshot_pixel_logical(
    screenshot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
) -> Option<[u8; 4]> {
    let logical_width = screenshot_logical_width(screenshot);
    let logical_height = screenshot_logical_height(screenshot);
    if x < 0.0 || y < 0.0 || x > logical_width || y > logical_height {
        return None;
    }

    sample_screenshot_pixel_bilinear(
        screenshot,
        logical_to_screenshot_x(screenshot, x),
        logical_to_screenshot_y(screenshot, y),
    )
}

pub fn logical_region_to_pixel_bounds(
    screenshot: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
) -> Option<(u32, u32, u32, u32)> {
    if region.2 <= 0.0 || region.3 <= 0.0 || screenshot.width == 0 || screenshot.height == 0 {
        return None;
    }

    let left = logical_to_screenshot_x(screenshot, region.0.max(0.0))
        .floor()
        .max(0.0) as u32;
    let top = logical_to_screenshot_y(screenshot, region.1.max(0.0))
        .floor()
        .max(0.0) as u32;
    let right = logical_to_screenshot_x(
        screenshot,
        (region.0 + region.2).min(screenshot_logical_width(screenshot)),
    )
    .ceil()
    .min(screenshot.width as f32) as u32;
    let bottom = logical_to_screenshot_y(
        screenshot,
        (region.1 + region.3).min(screenshot_logical_height(screenshot)),
    )
    .ceil()
    .min(screenshot.height as f32) as u32;

    if right <= left || bottom <= top {
        return None;
    }

    Some((left, top, right, bottom))
}

/// Crops a rectangular region from a screenshot.
pub fn crop_screenshot(
    screenshot: &cranpose::RobotScreenshot,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Option<cranpose::RobotScreenshot> {
    if width == 0 || height == 0 {
        return None;
    }
    let end_x = x.checked_add(width)?;
    let end_y = y.checked_add(height)?;
    if end_x > screenshot.width || end_y > screenshot.height {
        return None;
    }

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for row in 0..height {
        let src_start = (((y + row) * screenshot.width + x) * 4) as usize;
        let src_end = src_start + (width * 4) as usize;
        let dst_start = (row * width * 4) as usize;
        let dst_end = dst_start + (width * 4) as usize;
        pixels[dst_start..dst_end].copy_from_slice(&screenshot.pixels[src_start..src_end]);
    }

    Some(cranpose::RobotScreenshot {
        width,
        height,
        logical_width: width as f32 / screenshot_scale_x(screenshot),
        logical_height: height as f32 / screenshot_scale_y(screenshot),
        pixels,
    })
}

pub fn crop_screenshot_logical(
    screenshot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<cranpose::RobotScreenshot> {
    let (left, top, right, bottom) =
        logical_region_to_pixel_bounds(screenshot, (x, y, width, height))?;
    crop_screenshot(screenshot, left, top, right - left, bottom - top)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotPixelDifference {
    pub x: u32,
    pub y: u32,
    pub before: [u8; 4],
    pub after: [u8; 4],
    pub difference: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotDifferenceStats {
    pub differing_pixels: usize,
    pub max_difference: u32,
    pub first_difference: Option<ScreenshotPixelDifference>,
}

/// Normalize a floating-point screenshot region into a stable pixel grid with bilinear sampling.
///
/// This is useful for comparing the local picture of a translated subtree after compensating for
/// parent motion.
pub fn normalize_screenshot_region(
    screenshot: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    output_width: u32,
    output_height: u32,
) -> Option<cranpose::RobotScreenshot> {
    if output_width == 0
        || output_height == 0
        || region.2 <= 0.0
        || region.3 <= 0.0
        || screenshot.width == 0
        || screenshot.height == 0
    {
        return None;
    }

    let mut pixels = Vec::with_capacity((output_width * output_height * 4) as usize);
    for y in 0..output_height {
        for x in 0..output_width {
            let sample_x = region.0 + ((x as f32 + 0.5) * region.2 / output_width as f32);
            let sample_y = region.1 + ((y as f32 + 0.5) * region.3 / output_height as f32);
            pixels.extend_from_slice(&sample_screenshot_pixel_logical(
                screenshot, sample_x, sample_y,
            )?);
        }
    }

    Some(cranpose::RobotScreenshot {
        width: output_width,
        height: output_height,
        logical_width: output_width as f32,
        logical_height: output_height as f32,
        pixels,
    })
}

/// Count differing pixels and report the strongest difference between two screenshots of equal
/// dimensions. The difference metric is the maximum per-channel absolute difference.
pub fn screenshot_difference_stats(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    difference_tolerance: u32,
) -> Option<ScreenshotDifferenceStats> {
    if before.width != after.width || before.height != after.height {
        return None;
    }

    let mut differing_pixels = 0usize;
    let mut max_difference = 0u32;
    let mut first_difference = None;

    for y in 0..before.height {
        for x in 0..before.width {
            let before_pixel = screenshot_pixel(before, x, y)?;
            let after_pixel = screenshot_pixel(after, x, y)?;
            let difference = pixel_difference(before_pixel, after_pixel);
            if difference > difference_tolerance {
                differing_pixels += 1;
                max_difference = max_difference.max(difference);
                if first_difference.is_none() {
                    first_difference = Some(ScreenshotPixelDifference {
                        x,
                        y,
                        before: before_pixel,
                        after: after_pixel,
                        difference,
                    });
                }
            }
        }
    }

    Some(ScreenshotDifferenceStats {
        differing_pixels,
        max_difference,
        first_difference,
    })
}

/// Count pixels that differ between two screenshots by more than `channel_threshold`
/// on any RGBA channel.
pub fn changed_pixel_count(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    channel_threshold: u8,
) -> usize {
    let Some(expected_len) = expected_screenshot_pixel_bytes(before) else {
        return usize::MAX;
    };
    if before.width != after.width
        || before.height != after.height
        || !screenshot_pixel_storage_complete(before)
        || !screenshot_pixel_storage_complete(after)
    {
        return usize::MAX;
    }

    before.pixels[..expected_len]
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.pixels[..expected_len].as_chunks::<4>().0)
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > channel_threshold
                || a[1].abs_diff(b[1]) > channel_threshold
                || a[2].abs_diff(b[2]) > channel_threshold
                || a[3].abs_diff(b[3]) > channel_threshold
        })
        .count()
}

/// Count pixels that differ within a sub-region (x, y, w, h) of two screenshots.
pub fn changed_pixel_count_in_region(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    region: (f32, f32, f32, f32),
    channel_threshold: u8,
) -> usize {
    if before.width != after.width
        || before.height != after.height
        || !screenshot_pixel_storage_complete(before)
        || !screenshot_pixel_storage_complete(after)
        || (screenshot_scale_x(before) - screenshot_scale_x(after)).abs() > f32::EPSILON
        || (screenshot_scale_y(before) - screenshot_scale_y(after)).abs() > f32::EPSILON
    {
        return usize::MAX;
    }

    let Some((left, top, right, bottom)) = logical_region_to_pixel_bounds(before, region) else {
        return 0;
    };

    let width = before.width as usize;
    let mut changed = 0usize;
    for y in top..bottom {
        for x in left..right {
            let idx = ((y as usize) * width + x as usize) * 4;
            if before.pixels[idx].abs_diff(after.pixels[idx]) > channel_threshold
                || before.pixels[idx + 1].abs_diff(after.pixels[idx + 1]) > channel_threshold
                || before.pixels[idx + 2].abs_diff(after.pixels[idx + 2]) > channel_threshold
                || before.pixels[idx + 3].abs_diff(after.pixels[idx + 3]) > channel_threshold
            {
                changed += 1;
            }
        }
    }

    changed
}

fn sample_screenshot_pixel_bilinear(
    screenshot: &cranpose::RobotScreenshot,
    x: f32,
    y: f32,
) -> Option<[u8; 4]> {
    let max_x = screenshot.width.saturating_sub(1) as f32;
    let max_y = screenshot.height.saturating_sub(1) as f32;
    let source_x = (x - 0.5).clamp(0.0, max_x);
    let source_y = (y - 0.5).clamp(0.0, max_y);
    let x0 = source_x.floor() as u32;
    let y0 = source_y.floor() as u32;
    let x1 = (x0 + 1).min(screenshot.width.saturating_sub(1));
    let y1 = (y0 + 1).min(screenshot.height.saturating_sub(1));
    let tx = source_x - x0 as f32;
    let ty = source_y - y0 as f32;
    let top_left = screenshot_pixel(screenshot, x0, y0)?;
    let top_right = screenshot_pixel(screenshot, x1, y0)?;
    let bottom_left = screenshot_pixel(screenshot, x0, y1)?;
    let bottom_right = screenshot_pixel(screenshot, x1, y1)?;

    let lerp_channel = |index: usize| {
        let top = top_left[index] as f32 * (1.0 - tx) + top_right[index] as f32 * tx;
        let bottom = bottom_left[index] as f32 * (1.0 - tx) + bottom_right[index] as f32 * tx;
        (top * (1.0 - ty) + bottom * ty).round() as u8
    };

    Some([
        lerp_channel(0),
        lerp_channel(1),
        lerp_channel(2),
        lerp_channel(3),
    ])
}

fn pixel_difference(before: [u8; 4], after: [u8; 4]) -> u32 {
    before
        .into_iter()
        .zip(after)
        .map(|(lhs, rhs)| lhs.abs_diff(rhs) as u32)
        .max()
        .unwrap_or(0)
}

/// Parse "label: value" text from a slider label, returning the numeric value.
pub fn parse_slider_value(text: &str) -> Option<f32> {
    text.split_once(':')
        .and_then(|(_, value)| value.trim().parse::<f32>().ok())
}

/// Scroll down by dragging from `from_y` to `to_y` at `center_x`.
pub fn scroll_down(robot: &cranpose::Robot, center_x: f32, from_y: f32, to_y: f32) {
    let _ = robot.drag(center_x, from_y, center_x, to_y);
    std::thread::sleep(std::time::Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

/// Scroll up by dragging from `from_y` to `to_y` at `center_x`.
pub fn scroll_up(robot: &cranpose::Robot, center_x: f32, from_y: f32, to_y: f32) {
    let _ = robot.drag(center_x, from_y, center_x, to_y);
    std::thread::sleep(std::time::Duration::from_millis(180));
    let _ = robot.wait_for_idle();
}

/// Check whether a given Y coordinate falls within the visible viewport
/// (with 28px margin top and bottom).
pub fn y_is_visible(robot: &cranpose::Robot, y: f32) -> bool {
    let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
        return true;
    };
    let top = root_y + 28.0;
    let bottom = root_y + root_h - 28.0;
    y >= top && y <= bottom
}

fn scroll_toward_y(robot: &cranpose::Robot, y: f32, cfg: ScrollConfig) {
    let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
        return;
    };
    let viewport_mid = root_y + root_h * 0.5;
    let distance = ((y - viewport_mid).abs() / 5.0).clamp(24.0, 140.0);
    if y > viewport_mid {
        let target_y = (cfg.down_from_y - distance).max(cfg.down_to_y);
        let _ = robot.drag(cfg.center_x, cfg.down_from_y, cfg.center_x, target_y);
    } else {
        let target_y = (cfg.up_from_y + distance).min(cfg.up_to_y);
        let _ = robot.drag(cfg.center_x, cfg.up_from_y, cfg.center_x, target_y);
    }
    std::thread::sleep(std::time::Duration::from_millis(140));
    let _ = robot.wait_for_idle();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MissingTargetScrollDirection {
    Down,
    Up,
}

fn missing_target_scroll_direction(attempt: usize) -> MissingTargetScrollDirection {
    if attempt % 4 == 3 {
        MissingTargetScrollDirection::Up
    } else {
        MissingTargetScrollDirection::Down
    }
}

fn scroll_for_missing_target(robot: &cranpose::Robot, cfg: ScrollConfig, attempt: usize) {
    match missing_target_scroll_direction(attempt) {
        MissingTargetScrollDirection::Down => {
            scroll_down(robot, cfg.center_x, cfg.down_from_y, cfg.down_to_y);
        }
        MissingTargetScrollDirection::Up => {
            scroll_up(robot, cfg.center_x, cfg.up_from_y, cfg.up_to_y);
        }
    }
}

/// Configuration for scroll-into-view helpers so we don't need too many arguments.
#[derive(Clone, Copy, Debug)]
pub struct ScrollConfig {
    pub center_x: f32,
    pub down_from_y: f32,
    pub down_to_y: f32,
    pub up_from_y: f32,
    pub up_to_y: f32,
}

/// Scroll until a semantics node with the given `prefix` text is visible.
/// Returns bounds + full text `(x, y, w, h, text)`.
pub fn scroll_prefix_into_view(
    robot: &cranpose::Robot,
    prefix: &str,
    max_attempts: usize,
    cfg: ScrollConfig,
) -> Option<(f32, f32, f32, f32, String)> {
    for attempt in 0..max_attempts {
        if let Some(bounds) = find_text_by_prefix_in_semantics(robot, prefix) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if y_is_visible(robot, center_y) {
                return Some(bounds);
            }
            scroll_toward_y(robot, center_y, cfg);
        } else {
            scroll_for_missing_target(robot, cfg, attempt);
        }
    }
    None
}

/// Scroll until a semantics node with exactly matching text is visible.
/// Returns bounds `(x, y, w, h)`.
pub fn scroll_text_into_view(
    robot: &cranpose::Robot,
    text: &str,
    max_attempts: usize,
    cfg: ScrollConfig,
) -> Option<(f32, f32, f32, f32)> {
    for attempt in 0..max_attempts {
        if let Some(bounds) = find_bounds_by_text(robot, text) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if y_is_visible(robot, center_y) {
                return Some(bounds);
            }
            scroll_toward_y(robot, center_y, cfg);
        } else {
            scroll_for_missing_target(robot, cfg, attempt);
        }
    }
    None
}

/// Set a slider to a given fraction [0, 1] and return the parsed value.
pub fn set_slider_fraction(
    robot: &cranpose::Robot,
    prefix: &str,
    fraction: f32,
    slider_width: f32,
    slider_touch_offset_y: f32,
    cfg: ScrollConfig,
) -> Option<f32> {
    let (x, y, _w, h, _) = scroll_prefix_into_view(robot, prefix, 18, cfg)?;
    let slider_y = y + h + slider_touch_offset_y;
    let left_x = x + 2.0;
    let target_x = x + slider_width * fraction.clamp(0.0, 1.0);
    let _ = robot.drag(left_x, slider_y, target_x, slider_y);
    std::thread::sleep(std::time::Duration::from_millis(120));
    let _ = robot.wait_for_idle();
    find_text_by_prefix_in_semantics(robot, prefix)
        .and_then(|(_, _, _, _, t)| parse_slider_value(&t))
}

#[cfg(test)]
mod tests {
    use cranpose::{RobotScreenshot, SemanticRect};

    use super::*;

    fn semantic_element(
        role: &str,
        text: Option<&str>,
        clickable: bool,
        bounds: RectBounds,
        children: Vec<SemanticElement>,
    ) -> SemanticElement {
        SemanticElement {
            role: role.to_string(),
            text: text.map(ToString::to_string),
            state_description: None,
            clickable,
            editable_text: false,
            text_selection: None,
            bounds: SemanticRect {
                x: bounds.0,
                y: bounds.1,
                width: bounds.2,
                height: bounds.3,
            },
            children,
        }
    }

    #[test]
    fn overflow_axis_direction_detects_horizontal_overflow() {
        let root = (0.0, 0.0, 300.0, 200.0);
        let target = (320.0, 20.0, 80.0, 30.0);
        assert_eq!(
            overflow_axis_direction(target, root),
            Some((TabAxis::Horizontal, -1.0))
        );
    }

    #[test]
    fn overflow_axis_direction_detects_vertical_overflow() {
        let root = (0.0, 0.0, 300.0, 200.0);
        let target = (20.0, -60.0, 80.0, 30.0);
        assert_eq!(
            overflow_axis_direction(target, root),
            Some((TabAxis::Vertical, 1.0))
        );
    }

    #[test]
    fn scroll_delta_for_overflow_moves_target_toward_visible_area() {
        let root = (0.0, 0.0, 300.0, 200.0);

        assert_eq!(
            scroll_delta_for_overflow((320.0, 20.0, 80.0, 30.0), root, TabAxis::Horizontal),
            Some((-104.0, 0.0))
        );
        assert_eq!(
            scroll_delta_for_overflow((-42.0, 20.0, 80.0, 30.0), root, TabAxis::Horizontal),
            Some((46.0, 0.0))
        );
        assert_eq!(
            scroll_delta_for_overflow((20.0, 190.0, 80.0, 30.0), root, TabAxis::Vertical),
            Some((0.0, -24.0))
        );
    }

    #[test]
    fn visible_bounds_accepts_subpixel_edge_fit() {
        let root = (0.0, 0.0, 1024.0, 768.0);
        let button = (861.17, 28.0, 158.83, 47.6);

        assert_eq!(visible_bounds(Some(button), root), Some(button));
        assert_eq!(
            scroll_delta_for_overflow(button, root, TabAxis::Horizontal),
            None
        );
    }

    #[test]
    fn visible_bounds_accepts_root_origin_button() {
        let root = (0.0, 0.0, 400.0, 300.0);
        let button = (0.0, 0.0, 92.33, 19.6);

        assert_eq!(visible_bounds(Some(button), root), Some(button));
        assert_eq!(overflow_axis_direction(button, root), None);
    }

    #[test]
    fn exact_button_matching_does_not_match_text_input_for_text_tab() {
        let text_input_tab = semantic_element(
            "Layout",
            None,
            true,
            (10.0, 20.0, 120.0, 40.0),
            vec![semantic_element(
                "Text",
                Some("Text Input"),
                false,
                (18.0, 28.0, 96.0, 24.0),
                Vec::new(),
            )],
        );

        assert!(
            find_button_by(&text_input_tab, "Text", text_contains).is_some(),
            "substring matching should keep the public contains behavior"
        );
        assert!(
            find_button_by(&text_input_tab, "Text", text_equals).is_none(),
            "exact tab matching must not treat Text Input as Text"
        );
    }

    #[test]
    fn scroll_anchor_prefers_edge_safe_button_over_clipped_edge_button() {
        let root = (0.0, 0.0, 1080.0, 820.0);
        let target = (1084.0, 28.0, 82.0, 47.6);
        let elements = vec![semantic_element(
            "Layout",
            None,
            false,
            root,
            vec![
                semantic_element(
                    "Layout",
                    Some("central"),
                    true,
                    (920.0, 28.0, 76.0, 47.6),
                    Vec::new(),
                ),
                semantic_element(
                    "Layout",
                    Some("edge"),
                    true,
                    (1019.0, 28.0, 56.0, 47.6),
                    Vec::new(),
                ),
            ],
        )];

        assert_eq!(
            find_scroll_anchor(&elements, target, root, TabAxis::Horizontal),
            Some((920.0, 28.0, 76.0, 47.6))
        );
    }

    #[test]
    fn missing_target_scroll_searches_down_with_periodic_reverse() {
        let directions: Vec<_> = (0..8).map(missing_target_scroll_direction).collect();

        assert_eq!(
            directions,
            vec![
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Up,
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Down,
                MissingTargetScrollDirection::Up,
            ]
        );
    }

    #[test]
    fn find_scroll_anchor_prefers_cross_axis_overlap() {
        let root = (0.0, 0.0, 300.0, 200.0);
        let target = (320.0, 22.0, 80.0, 28.0);

        let same_row = semantic_element(
            "Layout",
            Some("same-row"),
            true,
            (120.0, 20.0, 80.0, 30.0),
            vec![],
        );
        let other_row = semantic_element(
            "Layout",
            Some("other-row"),
            true,
            (120.0, 130.0, 80.0, 30.0),
            vec![],
        );
        let root_elem = semantic_element("Layout", None, false, root, vec![same_row, other_row]);

        let anchor = find_scroll_anchor(&[root_elem], target, root, TabAxis::Horizontal)
            .expect("expected anchor");

        assert_eq!(anchor, (120.0, 20.0, 80.0, 30.0));
    }

    #[test]
    fn find_clickables_in_range_handles_nan_x_without_panicking() {
        let malformed = semantic_element(
            "Layout",
            None,
            true,
            (f32::NAN, 20.0, 80.0, 30.0),
            vec![semantic_element(
                "Text",
                Some("Malformed"),
                false,
                (f32::NAN, 24.0, 60.0, 18.0),
                vec![],
            )],
        );
        let finite = semantic_element(
            "Layout",
            None,
            true,
            (12.0, 20.0, 80.0, 30.0),
            vec![semantic_element(
                "Text",
                Some("Finite"),
                false,
                (16.0, 24.0, 60.0, 18.0),
                vec![],
            )],
        );

        let clickables = find_clickables_in_range(&[malformed, finite], 0.0, 40.0);

        assert_eq!(clickables.len(), 2);
        assert_eq!(clickables[0].0, "Finite");
        assert_eq!(clickables[1].0, "Malformed");
        assert!(clickables[1].1.is_nan());
    }

    #[test]
    fn find_button_exact_requires_full_text_match() {
        let exact_button = semantic_element(
            "Button",
            None,
            true,
            (10.0, 20.0, 90.0, 28.0),
            vec![
                semantic_element(
                    "Text",
                    Some("Text"),
                    false,
                    (14.0, 24.0, 30.0, 20.0),
                    vec![],
                ),
                semantic_element(
                    "Text",
                    Some("Text Input"),
                    false,
                    (46.0, 24.0, 48.0, 20.0),
                    vec![],
                ),
            ],
        );
        let partial_only_button = semantic_element(
            "Button",
            None,
            true,
            (120.0, 20.0, 90.0, 28.0),
            vec![semantic_element(
                "Text",
                Some("Text Input"),
                false,
                (124.0, 24.0, 48.0, 20.0),
                vec![],
            )],
        );

        assert_eq!(
            find_button_by(&exact_button, "Text", text_equals),
            Some((10.0, 20.0, 90.0, 28.0))
        );
        assert_eq!(
            find_button_by(&partial_only_button, "Text", text_equals),
            None
        );
    }

    #[test]
    fn choose_button_bounds_prefers_visible_match_over_offscreen_first_match() {
        let root = (0.0, 0.0, 320.0, 240.0);
        let candidates = vec![(420.0, 12.0, 80.0, 40.0), (24.0, 12.0, 80.0, 40.0)];

        assert_eq!(
            choose_button_bounds(&candidates, root),
            Some((24.0, 12.0, 80.0, 40.0))
        );
    }

    #[test]
    fn visible_bounds_rejects_offscreen_button_candidates() {
        let root = (0.0, 0.0, 320.0, 240.0);

        assert_eq!(
            visible_bounds(Some((12.0, 12.0, 80.0, 40.0)), root),
            Some((12.0, 12.0, 80.0, 40.0))
        );
        assert_eq!(visible_bounds(Some((340.0, 12.0, 80.0, 40.0)), root), None);
        assert_eq!(visible_bounds(Some((-90.0, 12.0, 80.0, 40.0)), root), None);
    }

    #[test]
    fn screenshot_pixel_reads_expected_value() {
        let screenshot = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(screenshot_pixel(&screenshot, 1, 0), Some([5, 6, 7, 8]));
    }

    #[test]
    fn screenshot_pixel_rejects_truncated_storage() {
        let screenshot = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![1, 2, 3, 4, 5, 6, 7],
        };

        assert_eq!(screenshot_pixel(&screenshot, 1, 0), None);
        assert_eq!(sample_screenshot_pixel_logical(&screenshot, 1.0, 0.0), None);
    }

    #[test]
    fn crop_screenshot_extracts_region() {
        let screenshot = RobotScreenshot {
            width: 3,
            height: 2,
            logical_width: 3.0,
            logical_height: 2.0,
            pixels: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17,
                18, 255,
            ],
        };
        let cropped = crop_screenshot(&screenshot, 1, 0, 2, 2).expect("crop");
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(cropped.logical_width, 2.0);
        assert_eq!(cropped.logical_height, 2.0);
        assert_eq!(
            cropped.pixels,
            vec![4, 5, 6, 255, 7, 8, 9, 255, 13, 14, 15, 255, 16, 17, 18, 255]
        );
    }

    #[test]
    fn normalize_screenshot_region_preserves_pixel_grid_at_native_size() {
        let screenshot = RobotScreenshot {
            width: 2,
            height: 2,
            logical_width: 2.0,
            logical_height: 2.0,
            pixels: vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
        };

        let normalized =
            normalize_screenshot_region(&screenshot, (0.0, 0.0, 2.0, 2.0), 2, 2).expect("norm");

        assert_eq!(normalized.width, screenshot.width);
        assert_eq!(normalized.height, screenshot.height);
        assert_eq!(normalized.logical_width, screenshot.logical_width);
        assert_eq!(normalized.logical_height, screenshot.logical_height);
        assert_eq!(normalized.pixels, screenshot.pixels);
    }

    #[test]
    fn changed_pixel_count_in_region_uses_logical_coordinates() {
        let before = RobotScreenshot {
            width: 4,
            height: 4,
            logical_width: 2.0,
            logical_height: 2.0,
            pixels: vec![
                0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
                0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
                0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
            ],
        };
        let mut after = before.clone();
        for y in 2..4 {
            for x in 2..4 {
                let idx = ((y * after.width + x) * 4) as usize;
                after.pixels[idx] = 255;
            }
        }

        assert_eq!(
            changed_pixel_count_in_region(&before, &after, (1.0, 1.0, 1.0, 1.0), 1),
            4
        );
    }

    #[test]
    fn sample_screenshot_pixel_logical_maps_scaled_capture() {
        let screenshot = RobotScreenshot {
            width: 4,
            height: 4,
            logical_width: 2.0,
            logical_height: 2.0,
            pixels: vec![
                1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
                7, 0, 0, 255, 8, 0, 0, 255, 9, 0, 0, 255, 10, 0, 0, 255, 11, 0, 0, 255, 12, 0, 0,
                255, 13, 0, 0, 255, 14, 0, 0, 255, 15, 0, 0, 255, 16, 0, 0, 255,
            ],
        };

        assert_eq!(
            sample_screenshot_pixel_logical(&screenshot, 1.25, 1.25),
            Some([11, 0, 0, 255])
        );
    }

    #[test]
    fn screenshot_difference_stats_reports_first_difference() {
        let before = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![10, 20, 30, 255, 1, 2, 3, 255],
        };
        let after = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![10, 20, 30, 255, 4, 8, 3, 200],
        };

        let stats = screenshot_difference_stats(&before, &after, 3).expect("stats");

        assert_eq!(stats.differing_pixels, 1);
        assert_eq!(stats.max_difference, 55);
        assert_eq!(
            stats.first_difference,
            Some(ScreenshotPixelDifference {
                x: 1,
                y: 0,
                before: [1, 2, 3, 255],
                after: [4, 8, 3, 200],
                difference: 55,
            })
        );
    }

    #[test]
    fn screenshot_difference_stats_rejects_truncated_storage() {
        let before = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![10, 20, 30, 255, 1, 2, 3],
        };
        let after = RobotScreenshot {
            width: 2,
            height: 1,
            logical_width: 2.0,
            logical_height: 1.0,
            pixels: vec![10, 20, 30, 255, 4, 8, 3, 200],
        };

        assert_eq!(screenshot_difference_stats(&before, &after, 3), None);
        assert_eq!(changed_pixel_count(&before, &after, 3), usize::MAX);
        assert_eq!(
            changed_pixel_count_in_region(&before, &after, (0.0, 0.0, 2.0, 1.0), 3),
            usize::MAX
        );
    }
}
