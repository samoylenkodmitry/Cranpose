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

/// Maximum time between taps that still counts as a multi-tap, in milliseconds.
pub const MULTI_TAP_TIMEOUT_MS: u128 = 500;

/// Maximum distance (px) between consecutive taps that still counts as a
/// multi-tap. A tap that lands far from the previous one starts a fresh
/// single tap even if it arrives quickly, matching Android's `ViewConfiguration`
/// double-tap slop behavior.
pub const MULTI_TAP_SLOP_PX: f32 = 24.0;

/// The unit of text a tap gesture selects, growing with the tap count the way
/// mature text editors do (Android `TextView`, iOS `UITextView`, VS Code):
///
/// * 1 tap → [`Caret`](SelectionGranularity::Caret) (place the cursor);
/// * 2 taps → [`Word`](SelectionGranularity::Word);
/// * 3 taps → [`Line`](SelectionGranularity::Line);
/// * 4 taps → [`Paragraph`](SelectionGranularity::Paragraph);
/// * 5+ taps → cycle back through word → line → paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionGranularity {
    /// Collapsed caret (a single tap places the cursor).
    Caret,
    /// The word under the tap.
    Word,
    /// The line under the tap (delimited by `\n`).
    Line,
    /// The paragraph under the tap (delimited by blank lines).
    Paragraph,
}

/// Classifies a press into a 1-based tap count from the previous tap's count,
/// the time since it, and the distance from it.
///
/// `previous` is the last tap's `(count, x, y)` or `None` for the first tap. A
/// tap increments the count only when it lands within both the timeout and the
/// slop radius; otherwise it restarts at `1`. The count is **not** wrapped here
/// — the granularity mapping ([`tap_selection_granularity`]) cycles instead, so
/// the field can keep escalating (word → line → paragraph → word …) as long as
/// the finger keeps tapping in place.
pub fn classify_tap_count(
    previous: Option<(u8, f32, f32)>,
    elapsed_ms: u128,
    x: f32,
    y: f32,
    timeout_ms: u128,
    slop_px: f32,
) -> u8 {
    let Some((prev_count, prev_x, prev_y)) = previous else {
        return 1;
    };
    let within_time = elapsed_ms <= timeout_ms;
    let dx = x - prev_x;
    let dy = y - prev_y;
    let within_slop = dx * dx + dy * dy <= slop_px * slop_px;
    if !within_time || !within_slop {
        return 1;
    }
    prev_count.saturating_add(1)
}

/// Maps a 1-based tap count to the granularity it selects.
///
/// A single tap places the caret; two taps select the word, three the line,
/// four the paragraph, and every further tap cycles back through
/// word → line → paragraph so a resting finger keeps toggling between the three
/// range granularities (matching desktop editors and iOS).
pub fn tap_selection_granularity(tap_count: u8) -> SelectionGranularity {
    match tap_count {
        0 | 1 => SelectionGranularity::Caret,
        n => match (n - 2) % 3 {
            0 => SelectionGranularity::Word,
            1 => SelectionGranularity::Line,
            _ => SelectionGranularity::Paragraph,
        },
    }
}

/// Returns the byte range `[start, end)` of the line containing `pos`, delimited
/// by `\n` (the newline itself is excluded from the range).
///
/// Used for triple-tap line selection. Byte offsets always land on `char`
/// boundaries because `\n` is a single-byte ASCII character.
pub fn find_line_boundaries(text: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(text.len());
    let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[pos..]
        .find('\n')
        .map(|i| pos + i)
        .unwrap_or(text.len());
    (start, end)
}

/// Returns the byte range `[start, end)` of the paragraph containing `pos`.
///
/// Paragraphs are delimited by blank lines — a run of two or more consecutive
/// `\n` — so a fourth tap grows the selection from one line to the whole block
/// of text around it. Text with no blank line is a single paragraph (the whole
/// string). Byte offsets land on `char` boundaries because `\n` is single-byte
/// ASCII. Unicode-aware: multi-byte characters inside the paragraph are spanned
/// whole.
pub fn find_paragraph_boundaries(text: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(text.len());
    // Start: just after the last blank-line separator at or before `pos`.
    let start = text[..pos]
        .rfind("\n\n")
        .map(|i| {
            // Skip the whole run of blank lines so the paragraph starts on its
            // first non-empty line.
            let mut s = i + 1;
            while text[s..].starts_with('\n') {
                s += 1;
            }
            s
        })
        .unwrap_or(0);
    // End: the next blank-line separator at or after `pos`.
    let end = text[pos..]
        .find("\n\n")
        .map(|i| pos + i)
        .unwrap_or(text.len());
    (start.min(end), end)
}

/// Which selection handle a teardrop represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            // Android start (left) handle: the point sits at the TOP-RIGHT
            // (touching the selection start) with a straight vertical right edge,
            // and the round bulb hangs down and to the LEFT. Traced tip → straight
            // down the right edge → 270° arc round the bulb (whose centre sits at
            // `(tip_x - r, cy)`, down-left of the tip) → back along the top edge to
            // the tip.
            //
            // The sweep flag is `1` (clockwise, y-down): together with the
            // large-arc flag this centres the arc on the bulb below-left of the
            // tip. Sweep `0` would instead centre the arc on the tip itself,
            // drawing an upward pac-man wedge that overlaps the glyph line — the
            // reported "inverted teardrop".
            format!(
                "M {tip_x} {tip_y} L {tip_x} {cy} A {r} {r} 0 1 1 {left} {tip_y} Z",
                left = tip_x - r,
            )
        }
        HandleKind::SelectionEnd => {
            // Android end (right) handle: the exact mirror of the start handle —
            // the point sits at the TOP-LEFT (touching the selection end) with a
            // straight vertical left edge, and the round bulb hangs down and to
            // the RIGHT. Same trace as the start handle with the arc swept the
            // other way (sweep `0`) so it is a true reflection (not rotated): the
            // bulb centre sits at `(tip_x + r, cy)`, down-right of the tip.
            format!(
                "M {tip_x} {tip_y} L {tip_x} {cy} A {r} {r} 0 1 0 {right} {tip_y} Z",
                right = tip_x + r,
            )
        }
    }
}

/// Finger-sized grab slop (px) added around a handle's drawn teardrop to enlarge
/// its touch target, matching Android's generous handle hit area. A bare
/// teardrop (~2·[`HANDLE_RADIUS`] across) is far smaller than a fingertip, so a
/// touch-DOWN aimed at a handle routinely lands a few px off it; without this
/// slop the press falls through to the field below and places a caret, which
/// collapses the selection. The slop is applied to the sides and BELOW the tip
/// (where the bulb and the grabbing finger sit) but never ABOVE the tip — see
/// [`crate::widgets::selection_handle`], which keeps the box off the glyph line
/// so a double-tap still reaches the field to escalate into a word selection.
pub const HANDLE_GRAB_SLOP: f32 = 24.0;

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
        assert_eq!(classify_tap_count(None, 0, 10.0, 10.0, 500, 24.0), 1);
        assert_eq!(
            classify_tap_count(Some((1, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
            2
        );
        assert_eq!(
            classify_tap_count(Some((2, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
            3
        );
        // A fourth in-place tap keeps counting up (the granularity mapping is
        // what cycles, not the raw count).
        assert_eq!(
            classify_tap_count(Some((3, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
            4
        );
        assert_eq!(
            classify_tap_count(Some((4, 10.0, 10.0)), 100, 11.0, 12.0, 500, 24.0),
            5
        );
    }

    #[test]
    fn tap_classification_resets_past_timeout_or_slop() {
        // Too slow: restarts.
        assert_eq!(
            classify_tap_count(Some((1, 10.0, 10.0)), 600, 10.0, 10.0, 500, 24.0),
            1
        );
        // Too far: restarts even though it is quick.
        assert_eq!(
            classify_tap_count(Some((1, 10.0, 10.0)), 50, 100.0, 10.0, 500, 24.0),
            1
        );
        // A reset also applies from a higher count.
        assert_eq!(
            classify_tap_count(Some((3, 10.0, 10.0)), 600, 10.0, 10.0, 500, 24.0),
            1
        );
    }

    #[test]
    fn tap_granularity_grows_then_cycles() {
        use SelectionGranularity::*;
        assert_eq!(tap_selection_granularity(0), Caret);
        assert_eq!(tap_selection_granularity(1), Caret);
        assert_eq!(tap_selection_granularity(2), Word);
        assert_eq!(tap_selection_granularity(3), Line);
        assert_eq!(tap_selection_granularity(4), Paragraph);
        // Fifth tap cycles back to word, then line, then paragraph again.
        assert_eq!(tap_selection_granularity(5), Word);
        assert_eq!(tap_selection_granularity(6), Line);
        assert_eq!(tap_selection_granularity(7), Paragraph);
        assert_eq!(tap_selection_granularity(8), Word);
    }

    #[test]
    fn paragraph_boundaries_span_blank_line_delimited_blocks() {
        let text = "line one\nline two\n\nsecond para\nstill second\n\n\nthird";
        // Inside the first paragraph (two lines).
        let (s, e) = find_paragraph_boundaries(text, 3);
        assert_eq!(&text[s..e], "line one\nline two");
        // Inside the second paragraph.
        let (s, e) = find_paragraph_boundaries(text, 20);
        assert_eq!(&text[s..e], "second para\nstill second");
        // Inside the third paragraph, after a run of THREE newlines.
        let (s, e) = find_paragraph_boundaries(text, text.len());
        assert_eq!(&text[s..e], "third");
    }

    #[test]
    fn paragraph_boundaries_no_blank_line_is_whole_text() {
        let text = "just\none\nblock";
        assert_eq!(find_paragraph_boundaries(text, 5), (0, text.len()));
    }

    #[test]
    fn paragraph_boundaries_are_unicode_aware() {
        // Multi-byte characters must be spanned whole and offsets stay on char
        // boundaries.
        let text = "\u{4e2d}\u{6587}\u{6bb5}\u{843d}\n\n\u{6b21}";
        let first = "\u{4e2d}\u{6587}\u{6bb5}\u{843d}";
        let (s, e) = find_paragraph_boundaries(text, 3);
        assert_eq!(&text[s..e], first);
        assert!(text.is_char_boundary(s) && text.is_char_boundary(e));
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

    /// Regression for the inverted-teardrop bug: the drawn selection handles
    /// must have the correct Android orientation — the whole teardrop sits AT OR
    /// BELOW the tip line (never above it, into the glyphs), and each handle's
    /// bulb hangs to the correct side of its tip:
    ///
    /// * start (left) handle — point top-right, bulb down-LEFT (all geometry at
    ///   or left of the tip's x);
    /// * end (right) handle — point top-left, bulb down-RIGHT (all geometry at or
    ///   right of the tip's x);
    /// * cursor handle — symmetric, centred on the tip.
    ///
    /// A sweep-flag mistake used to centre the arc on the tip, producing an
    /// upward pac-man wedge that extended ABOVE the tip and to the wrong side —
    /// exactly what this guards against.
    #[test]
    fn selection_handles_point_at_the_tip_with_the_bulb_below() {
        let (tip_x, tip_y, r) = (40.0_f32, 20.0_f32, HANDLE_RADIUS);
        let eps = 0.5_f32;

        let sample_points = |kind: HandleKind| -> Vec<cranpose_ui_graphics::Point> {
            let data = handle_path_data(kind, tip_x, tip_y, r);
            let path = cranpose_ui_graphics::VectorPath::parse(&data).expect("valid handle path");
            path.subpaths().iter().flatten().copied().collect()
        };

        // No handle draws any geometry ABOVE the tip line — that region belongs to
        // the glyphs, and a handle poking up into it is the inverted-teardrop bug.
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            for p in sample_points(kind) {
                assert!(
                    p.y >= tip_y - eps,
                    "{kind:?}: point {p:?} is above the tip line y={tip_y} (teardrop inverted)"
                );
            }
        }

        // Start bulb hangs down-LEFT: every point is at or left of the tip's x,
        // and the shape reaches a full bulb-width to the left.
        let start = sample_points(HandleKind::SelectionStart);
        assert!(
            start.iter().all(|p| p.x <= tip_x + eps),
            "start handle must not extend right of its tip"
        );
        assert!(
            start.iter().any(|p| p.x <= tip_x - 2.0 * r + eps),
            "start handle bulb must reach a full diameter to the LEFT of the tip"
        );

        // End bulb hangs down-RIGHT: mirror image of the start handle.
        let end = sample_points(HandleKind::SelectionEnd);
        assert!(
            end.iter().all(|p| p.x >= tip_x - eps),
            "end handle must not extend left of its tip"
        );
        assert!(
            end.iter().any(|p| p.x >= tip_x + 2.0 * r - eps),
            "end handle bulb must reach a full diameter to the RIGHT of the tip"
        );

        // Cursor handle is symmetric about the tip: it reaches ~r to each side.
        let cursor = sample_points(HandleKind::Cursor);
        assert!(
            cursor.iter().any(|p| p.x <= tip_x - r + eps)
                && cursor.iter().any(|p| p.x >= tip_x + r - eps),
            "cursor handle must be symmetric about the tip"
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
