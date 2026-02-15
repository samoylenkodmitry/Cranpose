//! Common helper functions for robot tests
//!
//! These helpers work with `cranpose::SemanticElement` to find and interact
//! with UI elements during robot testing.

use crate::robot_assertions::{Bounds, SemanticElementLike};
use cranpose::SemanticElement;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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
    let text_owned = text.to_string();
    let mut bounds = find_in_semantics(robot, |elem| find_button(elem, &text_owned));

    let Some(root) = root_bounds(robot) else {
        return bounds;
    };

    if let Some(current) = bounds {
        if is_fully_visible(current, root) {
            return bounds;
        }
    } else {
        return None;
    }

    for _ in 0..8 {
        let Some(current) = bounds else {
            break;
        };

        let Some((axis, dir)) = overflow_axis_direction(current, root) else {
            break;
        };

        let semantics = match robot.get_semantics() {
            Ok(semantics) => semantics,
            Err(e) => {
                eprintln!("  ✗ Failed to get semantics: {}", e);
                break;
            }
        };

        let Some((sx, sy, sw, sh)) = find_scroll_anchor(&semantics, current, root, axis) else {
            break;
        };

        let start_x = sx + sw / 2.0;
        let start_y = sy + sh / 2.0;
        let (end_x, end_y) = match axis {
            TabAxis::Horizontal => (start_x + dir * SCROLL_STEP, start_y),
            TabAxis::Vertical => (start_x, start_y + dir * SCROLL_STEP),
        };
        let _ = robot.drag(start_x, start_y, end_x, end_y);
        std::thread::sleep(Duration::from_millis(SCROLL_SETTLE_MS));
        let _ = robot.wait_for_idle();

        bounds = find_in_semantics(robot, |elem| find_button(elem, &text_owned));
        if let Some(current) = bounds {
            if is_fully_visible(current, root) {
                break;
            }
        }
    }

    bounds
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

pub fn find_center_by_text(robot: &cranpose::Robot, text: &str) -> Option<(f32, f32)> {
    let (x, y, w, h) = find_bounds_by_text(robot, text)?;
    Some((x + w / 2.0, y + h / 2.0))
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
        if let Some(text) = elem.text.as_deref() {
            if text.starts_with(prefix) {
                *counts.entry(text.to_string()).or_insert(0) += 1;
            }
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
        if let Some(bounds) = find_in_semantics(robot, |elem| find_button(elem, label)) {
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
const SCROLL_STEP: f32 = 240.0;
const SCROLL_SETTLE_MS: u64 = 140;

fn is_axis_visible(bounds: RectBounds, root: RectBounds, axis: TabAxis) -> bool {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;
    match axis {
        TabAxis::Horizontal => {
            x >= rx + VISIBILITY_PADDING && x + w <= rx + rw - VISIBILITY_PADDING
        }
        TabAxis::Vertical => y >= ry + VISIBILITY_PADDING && y + h <= ry + rh - VISIBILITY_PADDING,
    }
}

fn is_fully_visible(bounds: RectBounds, root: RectBounds) -> bool {
    is_axis_visible(bounds, root, TabAxis::Horizontal)
        && is_axis_visible(bounds, root, TabAxis::Vertical)
}

fn overflow_axis_direction(bounds: RectBounds, root: RectBounds) -> Option<(TabAxis, f32)> {
    let (x, y, w, h) = bounds;
    let (rx, ry, rw, rh) = root;

    let overflow_left = (rx + VISIBILITY_PADDING - x).max(0.0);
    let overflow_right = (x + w - (rx + rw - VISIBILITY_PADDING)).max(0.0);
    let overflow_top = (ry + VISIBILITY_PADDING - y).max(0.0);
    let overflow_bottom = (y + h - (ry + rh - VISIBILITY_PADDING)).max(0.0);

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

fn find_scroll_anchor(
    elements: &[SemanticElement],
    target: RectBounds,
    root: RectBounds,
    axis: TabAxis,
) -> Option<RectBounds> {
    let mut best: Option<(RectBounds, f32, f32)> = None;

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
                    match best {
                        None => best = Some((bounds, overlap, distance)),
                        Some((_, best_overlap, best_distance)) => {
                            let is_better_overlap = overlap > best_overlap + f32::EPSILON;
                            let is_better_distance = (overlap - best_overlap).abs() <= f32::EPSILON
                                && distance < best_distance;
                            if is_better_overlap || is_better_distance {
                                best = Some((bounds, overlap, distance));
                            }
                        }
                    }
                }
            }
        }

        if let Some(found) = find_scroll_anchor(&elem.children, target, root, axis) {
            let overlap = cross_axis_overlap(found, target, axis);
            let distance = primary_axis_distance(found, target, axis);
            match best {
                None => best = Some((found, overlap, distance)),
                Some((_, best_overlap, best_distance)) => {
                    let is_better_overlap = overlap > best_overlap + f32::EPSILON;
                    let is_better_distance =
                        (overlap - best_overlap).abs() <= f32::EPSILON && distance < best_distance;
                    if is_better_overlap || is_better_distance {
                        best = Some((found, overlap, distance));
                    }
                }
            }
        }
    }

    best.map(|(bounds, _, _)| bounds)
}

/// Capture a full screenshot from the running robot app.
pub fn capture_screenshot(robot: &cranpose::Robot) -> Option<cranpose::RobotScreenshot> {
    robot.screenshot().ok()
}

/// Returns pixel RGBA at `(x, y)` from a screenshot.
pub fn screenshot_pixel(screenshot: &cranpose::RobotScreenshot, x: u32, y: u32) -> Option<[u8; 4]> {
    if x >= screenshot.width || y >= screenshot.height {
        return None;
    }
    let index = ((y * screenshot.width + x) * 4) as usize;
    Some([
        screenshot.pixels[index],
        screenshot.pixels[index + 1],
        screenshot.pixels[index + 2],
        screenshot.pixels[index + 3],
    ])
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
        pixels,
    })
}

/// Count pixels that differ between two screenshots by more than `channel_threshold`
/// on any RGBA channel.
pub fn changed_pixel_count(
    before: &cranpose::RobotScreenshot,
    after: &cranpose::RobotScreenshot,
    channel_threshold: u8,
) -> usize {
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    before
        .pixels
        .chunks_exact(4)
        .zip(after.pixels.chunks_exact(4))
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
    if before.width != after.width || before.height != after.height {
        return usize::MAX;
    }

    let left = region.0.max(0.0).floor() as u32;
    let top = region.1.max(0.0).floor() as u32;
    let right = (region.0 + region.2).min(before.width as f32).ceil() as u32;
    let bottom = (region.1 + region.3).min(before.height as f32).ceil() as u32;

    if right <= left || bottom <= top {
        return 0;
    }

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
            let Some((_, root_y, _, root_h)) = root_bounds(robot) else {
                return Some(bounds);
            };
            let viewport_mid = root_y + root_h * 0.5;
            if center_y > viewport_mid {
                scroll_down(robot, cfg.center_x, cfg.down_from_y, cfg.down_to_y);
            } else {
                scroll_up(robot, cfg.center_x, cfg.up_from_y, cfg.up_to_y);
            }
        } else {
            // Not found yet — alternate directions to find it
            if attempt % 2 == 0 {
                scroll_down(robot, cfg.center_x, cfg.down_from_y, cfg.down_to_y);
            } else {
                scroll_up(robot, cfg.center_x, cfg.up_from_y, cfg.up_to_y);
            }
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
    use super::*;
    use cranpose::{RobotScreenshot, SemanticRect};

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
            clickable,
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
    fn screenshot_pixel_reads_expected_value() {
        let screenshot = RobotScreenshot {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(screenshot_pixel(&screenshot, 1, 0), Some([5, 6, 7, 8]));
    }

    #[test]
    fn crop_screenshot_extracts_region() {
        let screenshot = RobotScreenshot {
            width: 3,
            height: 2,
            pixels: vec![
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17,
                18, 255,
            ],
        };
        let cropped = crop_screenshot(&screenshot, 1, 0, 2, 2).expect("crop");
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(
            cropped.pixels,
            vec![4, 5, 6, 255, 7, 8, 9, 255, 13, 14, 15, 255, 16, 17, 18, 255]
        );
    }
}
