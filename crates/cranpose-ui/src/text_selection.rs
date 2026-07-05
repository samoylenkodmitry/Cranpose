//! Native-grade text selection primitives for `BasicTextField`.
//!
//! This module holds the pure, unit-tested building blocks the text field uses
//! to offer Android/iOS-style selection: tap-count classification, word and
//! line/paragraph boundary detection, and the geometry of the draggable
//! teardrop selection handles (their shapes, their hit regions, and the
//! selection math that a handle drag produces).
//!
//! Keeping these as free functions makes the touch behavior testable without a
//! renderer and keeps `TextFieldModifierNode` focused on wiring.

use cranpose_ui_graphics::Rect;

/// How many consecutive taps a press represents, mirroring the platform text
/// selection gestures: one tap places the cursor, two select the word, three
/// select the line/paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapCount {
    Single,
    Double,
    Triple,
}

impl TapCount {
    /// The 1-based tap number, capped at three.
    pub fn as_u8(self) -> u8 {
        match self {
            TapCount::Single => 1,
            TapCount::Double => 2,
            TapCount::Triple => 3,
        }
    }
}

impl TryFrom<u8> for TapCount {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(TapCount::Single),
            2 => Ok(TapCount::Double),
            3 => Ok(TapCount::Triple),
            _ => Err(()),
        }
    }
}

/// Maximum time between taps that still counts as a multi-tap, in milliseconds.
pub const MULTI_TAP_TIMEOUT_MS: u128 = 500;

/// Maximum distance (px) between consecutive taps that still counts as a
/// multi-tap. A tap that lands far from the previous one starts a fresh
/// single tap even if it arrives quickly, matching Android's `ViewConfiguration`
/// double-tap slop behavior.
pub const MULTI_TAP_SLOP_PX: f32 = 24.0;

/// Classifies a press into a tap count from the previous tap's count, the time
/// since it, and the distance from it.
///
/// `previous` is the last tap's `(count, x, y)` or `None` for the first tap.
/// A tap escalates the count (single -> double -> triple, then wraps back to
/// single) only when it lands within both the timeout and the slop radius;
/// otherwise it restarts at a single tap.
pub fn classify_tap(
    previous: Option<(TapCount, f32, f32)>,
    elapsed_ms: u128,
    x: f32,
    y: f32,
    timeout_ms: u128,
    slop_px: f32,
) -> TapCount {
    let Some((prev_count, prev_x, prev_y)) = previous else {
        return TapCount::Single;
    };
    let within_time = elapsed_ms <= timeout_ms;
    let dx = x - prev_x;
    let dy = y - prev_y;
    let within_slop = dx * dx + dy * dy <= slop_px * slop_px;
    if !within_time || !within_slop {
        return TapCount::Single;
    }
    match prev_count {
        TapCount::Single => TapCount::Double,
        TapCount::Double => TapCount::Triple,
        // A fourth tap cycles back to a single cursor placement.
        TapCount::Triple => TapCount::Single,
    }
}

/// Returns the byte range `[start, end)` of the line/paragraph containing
/// `pos`, delimited by `\n` (the newline itself is excluded from the range).
///
/// Used for triple-tap line/paragraph selection. Byte offsets always land on
/// `char` boundaries because `\n` is a single-byte ASCII character.
pub fn find_line_boundaries(text: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(text.len());
    let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[pos..]
        .find('\n')
        .map(|i| pos + i)
        .unwrap_or(text.len());
    (start, end)
}

/// Which selection handle a teardrop represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    /// The blinking-cursor handle shown for a collapsed selection: a teardrop
    /// whose tip points up at the cursor, centered under it.
    Cursor,
    /// The start (leftmost) selection handle: tip at the top-right, bulb below-left.
    SelectionStart,
    /// The end (rightmost) selection handle: tip at the top-left, bulb below-right.
    SelectionEnd,
}

/// Radius of a selection/cursor handle bulb in px (Android uses ~11dp).
pub const HANDLE_RADIUS: f32 = 8.0;

/// Extra hit-test slop (px) around a handle so it is easy to grab with a finger.
pub const HANDLE_TOUCH_SLOP: f32 = 12.0;

/// SVG path data for a handle teardrop whose tip sits at `(tip_x, tip_y)`.
///
/// The tip is anchored at the text edge (the cursor position or a selection
/// endpoint at the line's bottom) and the rounded bulb hangs below it, so the
/// caller positions the handle by passing the on-screen anchor point.
pub fn handle_path_data(kind: HandleKind, tip_x: f32, tip_y: f32, radius: f32) -> String {
    let r = radius.max(0.0);
    let cy = tip_y + r; // bulb center y
    match kind {
        HandleKind::Cursor => {
            // Symmetric teardrop: tip up, full circular bulb below.
            format!(
                "M {tip_x} {tip_y} L {left} {cy} A {r} {r} 0 1 0 {right} {cy} Z",
                left = tip_x - r,
                right = tip_x + r,
            )
        }
        HandleKind::SelectionStart => {
            // Tip at the endpoint, bulb hanging down and to the LEFT.
            format!(
                "M {tip_x} {tip_y} L {tip_x} {cy} A {r} {r} 0 1 0 {left} {tip_y} Z",
                left = tip_x - r,
            )
        }
        HandleKind::SelectionEnd => {
            // Tip at the endpoint, bulb hanging down and to the RIGHT.
            format!(
                "M {tip_x} {tip_y} L {right} {tip_y} A {r} {r} 0 1 0 {tip_x} {cy} Z",
                right = tip_x + r,
            )
        }
    }
}

/// Extra padding (px) added around a handle's drawn teardrop to enlarge the
/// finger touch target, matching Android's generous handle hit area.
pub const HANDLE_TOUCH_PADDING: f32 = 12.0;

/// The axis-aligned hit region for a handle, expanded by touch slop, used to
/// decide whether a pointer-down grabbed a handle.
pub fn handle_hit_rect(kind: HandleKind, tip_x: f32, tip_y: f32, radius: f32, slop: f32) -> Rect {
    let r = radius.max(0.0);
    let slop = slop.max(0.0);
    // Horizontal span of the bulb relative to the tip depends on the handle side.
    let (left, right) = match kind {
        HandleKind::Cursor => (tip_x - r, tip_x + r),
        HandleKind::SelectionStart => (tip_x - 2.0 * r, tip_x + r),
        HandleKind::SelectionEnd => (tip_x - r, tip_x + 2.0 * r),
    };
    Rect {
        x: left - slop,
        y: tip_y - slop,
        width: (right - left) + 2.0 * slop,
        height: 2.0 * r + 2.0 * slop,
    }
}

/// Returns the handle nearest to `(x, y)` whose slop-expanded hit region
/// contains the point, or `None` when the point misses every handle.
///
/// `handles` lists the currently drawn handles as `(kind, tip_x, tip_y)`.
pub fn hit_test_handles(
    handles: &[(HandleKind, f32, f32)],
    x: f32,
    y: f32,
    radius: f32,
    slop: f32,
) -> Option<HandleKind> {
    let mut best: Option<(HandleKind, f32)> = None;
    for &(kind, tip_x, tip_y) in handles {
        let rect = handle_hit_rect(kind, tip_x, tip_y, radius, slop);
        let inside =
            x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height;
        if !inside {
            continue;
        }
        let cx = tip_x;
        let cy = tip_y + radius;
        let dist_sq = (x - cx) * (x - cx) + (y - cy) * (y - cy);
        if best
            .map(|(_, best_dist)| dist_sq < best_dist)
            .unwrap_or(true)
        {
            best = Some((kind, dist_sq));
        }
    }
    best.map(|(kind, _)| kind)
}

/// Computes the selection `(min, max)` that results from dragging one handle to
/// a new text `offset`, keeping the opposite (fixed) edge anchored.
///
/// Dragging never lets the two edges cross: a dragged start clamps to just
/// before the fixed end, and a dragged end clamps to just after the fixed
/// start, so the selection keeps at least one selected unit.
pub fn selection_after_handle_drag(
    dragged: HandleKind,
    fixed_edge: usize,
    dragged_offset: usize,
    text_len: usize,
) -> (usize, usize) {
    let fixed = fixed_edge.min(text_len);
    let dragged_offset = dragged_offset.min(text_len);
    match dragged {
        HandleKind::SelectionStart => {
            let start = dragged_offset.min(fixed.saturating_sub(1));
            (start, fixed)
        }
        HandleKind::SelectionEnd => {
            let end = dragged_offset.max(fixed + 1).min(text_len);
            (fixed, end)
        }
        // The cursor handle just moves the collapsed caret.
        HandleKind::Cursor => (dragged_offset, dragged_offset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_classification_escalates_within_time_and_slop() {
        assert_eq!(
            classify_tap(None, 0, 10.0, 10.0, 500, 24.0),
            TapCount::Single
        );
        assert_eq!(
            classify_tap(
                Some((TapCount::Single, 10.0, 10.0)),
                100,
                11.0,
                12.0,
                500,
                24.0
            ),
            TapCount::Double
        );
        assert_eq!(
            classify_tap(
                Some((TapCount::Double, 10.0, 10.0)),
                100,
                11.0,
                12.0,
                500,
                24.0
            ),
            TapCount::Triple
        );
        // A fourth quick tap wraps back to a single cursor placement.
        assert_eq!(
            classify_tap(
                Some((TapCount::Triple, 10.0, 10.0)),
                100,
                11.0,
                12.0,
                500,
                24.0
            ),
            TapCount::Single
        );
    }

    #[test]
    fn tap_classification_resets_past_timeout_or_slop() {
        // Too slow: restarts.
        assert_eq!(
            classify_tap(
                Some((TapCount::Single, 10.0, 10.0)),
                600,
                10.0,
                10.0,
                500,
                24.0
            ),
            TapCount::Single
        );
        // Too far: restarts even though it is quick.
        assert_eq!(
            classify_tap(
                Some((TapCount::Single, 10.0, 10.0)),
                50,
                100.0,
                10.0,
                500,
                24.0
            ),
            TapCount::Single
        );
    }

    #[test]
    fn line_boundaries_span_between_newlines() {
        let text = "first line\nsecond line\nthird";
        // Inside the second line.
        assert_eq!(find_line_boundaries(text, 15), (11, 22));
        // Start of the first line.
        assert_eq!(find_line_boundaries(text, 0), (0, 10));
        // Inside the last (newline-terminated-absent) line.
        assert_eq!(find_line_boundaries(text, 25), (23, text.len()));
    }

    #[test]
    fn line_boundaries_handle_unicode_and_empty_lines() {
        let text = "\u{00e9}\u{00e8}\n\n\u{4e2d}\u{6587}";
        // Empty middle line: start == end at the byte after the first newline.
        let (start, end) = find_line_boundaries(text, "\u{00e9}\u{00e8}\n".len());
        assert_eq!(start, end);
        // Last line spans the two CJK characters.
        let last = find_line_boundaries(text, text.len());
        assert_eq!(&text[last.0..last.1], "\u{4e2d}\u{6587}");
    }

    #[test]
    fn handle_path_is_non_empty_and_contains_the_tip() {
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let data = handle_path_data(kind, 40.0, 20.0, HANDLE_RADIUS);
            let path = cranpose_ui_graphics::VectorPath::parse(&data)
                .expect("handle path must be valid SVG");
            assert!(!path.is_empty(), "{kind:?} handle must have geometry");
            let bounds = path.bounds();
            // The tip (40, 20) must lie within the shape's bounds.
            assert!(bounds.x <= 40.0 + 0.5 && bounds.x + bounds.width >= 40.0 - 0.5);
            assert!(bounds.y <= 20.0 + 0.5);
            // The bulb hangs below the tip.
            assert!(bounds.y + bounds.height >= 20.0 + HANDLE_RADIUS);
        }
    }

    #[test]
    fn handle_hit_rect_covers_tip_and_bulb_with_slop() {
        let rect = handle_hit_rect(
            HandleKind::Cursor,
            40.0,
            20.0,
            HANDLE_RADIUS,
            HANDLE_TOUCH_SLOP,
        );
        // Tip and bulb center are inside.
        assert!(rect.x <= 40.0 && 40.0 <= rect.x + rect.width);
        assert!(rect.y <= 20.0 && 20.0 + HANDLE_RADIUS <= rect.y + rect.height);
        // Slop widens the region beyond the bulb radius.
        assert!(rect.width >= 2.0 * HANDLE_RADIUS + 2.0 * HANDLE_TOUCH_SLOP - 0.01);
    }

    #[test]
    fn hit_test_prefers_the_nearest_handle() {
        let handles = [
            (HandleKind::SelectionStart, 20.0, 20.0),
            (HandleKind::SelectionEnd, 120.0, 20.0),
        ];
        // Near the start handle bulb.
        assert_eq!(
            hit_test_handles(&handles, 20.0, 28.0, HANDLE_RADIUS, HANDLE_TOUCH_SLOP),
            Some(HandleKind::SelectionStart)
        );
        // Near the end handle bulb.
        assert_eq!(
            hit_test_handles(&handles, 120.0, 28.0, HANDLE_RADIUS, HANDLE_TOUCH_SLOP),
            Some(HandleKind::SelectionEnd)
        );
        // Far from both.
        assert_eq!(
            hit_test_handles(&handles, 300.0, 300.0, HANDLE_RADIUS, HANDLE_TOUCH_SLOP),
            None
        );
    }

    #[test]
    fn handle_drag_keeps_edges_from_crossing() {
        // Dragging the end handle left past the start clamps to start+1.
        assert_eq!(
            selection_after_handle_drag(HandleKind::SelectionEnd, 5, 2, 20),
            (5, 6)
        );
        // Dragging the end handle right extends normally.
        assert_eq!(
            selection_after_handle_drag(HandleKind::SelectionEnd, 5, 12, 20),
            (5, 12)
        );
        // Dragging the start handle right past the end clamps to end-1.
        assert_eq!(
            selection_after_handle_drag(HandleKind::SelectionStart, 8, 10, 20),
            (7, 8)
        );
        // Dragging the start handle left extends normally.
        assert_eq!(
            selection_after_handle_drag(HandleKind::SelectionStart, 8, 3, 20),
            (3, 8)
        );
        // The cursor handle moves a collapsed caret.
        assert_eq!(
            selection_after_handle_drag(HandleKind::Cursor, 4, 9, 20),
            (9, 9)
        );
    }
}
