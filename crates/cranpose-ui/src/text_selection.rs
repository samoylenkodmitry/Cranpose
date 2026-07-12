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

/// Resolves the effective tap count for a press, folding in the "tap inside an
/// existing selection" gesture so it drives the same word → line → paragraph
/// granularity ladder ([`tap_selection_granularity`]) as a rapid multi-tap.
///
/// Inputs:
/// * `raw_tap_count` — the time-and-slop-gated multi-tap count from
///   [`classify_tap_count`] (2+ means a genuine rapid multi-tap in progress);
/// * `previous_count` — the effective count the *previous* press resolved to
///   (the field remembers it as its click count);
/// * `tap_in_selection` — the press landed inside the current, non-collapsed
///   selection;
/// * `repeat_in_place` — the press landed within the multi-tap slop of the
///   previous press, **independent of timing** (the same spot, tapped again).
///
/// Behavior:
/// * a rapid multi-tap (`raw_tap_count >= 2`) uses its own running count, so
///   double→word, triple→line, … keep working exactly as before;
/// * a lone tap inside a selection selects the word under the finger, and each
///   further tap at the *same spot* climbs the ladder (word → line → paragraph →
///   word …) even when it arrives slowly (the multi-tap timeout has lapsed) —
///   users tap-then-look-then-tap, so the growth is keyed on location, not time;
/// * a lone tap at a *new* spot inside the selection re-grabs that word (resets
///   to word); and
/// * a lone tap outside any selection is left as-is (a single tap → caret).
pub fn resolve_selection_tap_count(
    raw_tap_count: u8,
    previous_count: u8,
    tap_in_selection: bool,
    repeat_in_place: bool,
) -> u8 {
    if raw_tap_count >= 2 {
        raw_tap_count
    } else if tap_in_selection {
        if repeat_in_place {
            // Keep climbing the granularity ladder at the same spot.
            previous_count.max(1).saturating_add(1)
        } else {
            // First tap inside the selection (or a tap on a different word):
            // grab the word under the finger.
            2
        }
    } else {
        raw_tap_count
    }
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

/// Which visual line a caret/handle at a soft-wrap boundary belongs to. At a
/// shared boundary byte (the end of one wrapped visual line IS the start of
/// the next — mid-word wraps produce these) the offset alone is ambiguous:
///
/// * [`LineAffinity::Upstream`] anchors to the END of the upper line — the
///   glyph a dragging finger means. Selection END and cursor handles, the
///   drawn caret, and the loupe use this; without it a drag along a wrapped
///   line's right edge snaps the handle one line DOWN and to the left edge.
/// * [`LineAffinity::Downstream`] anchors to the START of the lower line —
///   where the first selected glyph actually renders. Selection START handles
///   and highlight geometry use this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineAffinity {
    Upstream,
    Downstream,
}

/// Given the source byte ranges of the **visual** (wrapped) lines and a caret
/// byte `offset`, returns the `(visual_line_index, line_start_byte)` the caret
/// sits on.
///
/// The caret belongs to the last visual line whose start is at or before
/// `offset`, except at a shared soft-wrap boundary where `affinity` decides
/// (see [`LineAffinity`]):
/// * a caret in the middle of a visual line resolves to that line;
/// * a caret at the very end of the text sits on the last visual line.
///
/// This is the wrap-aware replacement for counting logical `\n` lines: without
/// it, a caret on a wrapped line's second visual line is drawn on the first (and
/// its x runs off the right edge), even though typing and the magnifier place it
/// correctly. Returns `(0, 0)` when there are no ranges.
pub fn caret_visual_line(
    ranges: &[std::ops::Range<usize>],
    offset: usize,
    affinity: LineAffinity,
) -> (usize, usize) {
    let mut result = (0usize, 0usize);
    for (index, range) in ranges.iter().enumerate() {
        if range.start <= offset {
            // A SHARED boundary (the previous line ends exactly where this one
            // starts — soft wrap, no separator byte) belongs upstream to the
            // upper line's end. A hard `\n` never shares (the ranges gap over
            // the separator), and an empty upper line never captures.
            if affinity == LineAffinity::Upstream
                && index > 0
                && range.start == offset
                && ranges[index - 1].end == offset
                && ranges[index - 1].start < offset
            {
                break;
            }
            result = (index, range.start);
        } else {
            break;
        }
    }
    result
}

/// Fraction of each DOWNWARD finger delta absorbed into the grab bias while
/// it drifts toward [`grab_bias_full_view`]: the handle starts under the
/// finger and drifts up-visible as the drag proceeds (the reference handle
/// "moves with the finger, then rides above it"), while the selection still
/// follows the remaining fraction — never a dead zone.
pub const GRAB_BIAS_DRIFT_FRACTION: f32 = 0.35;
/// Extra clearance (dp) below the handle dot once fully visible above the
/// finger.
pub const GRAB_BIAS_VIEW_CLEARANCE: f32 = 4.0;

/// The drift target: bias placing the finger just below the handle dot
/// (tip + dot + clearance), so the whole lollipop stays visible above it.
pub fn grab_bias_full_view() -> f32 {
    -(2.0 * HANDLE_RADIUS + GRAB_BIAS_VIEW_CLEARANCE)
}

/// One-way grab-bias ratchet: absorbs a fraction of DOWNWARD finger travel
/// into the bias until the handle rides fully visible above the finger;
/// upward travel never un-migrates (strict following once earned). Returns
/// the updated bias for a finger that moved from `last_y` to `now_y`.
pub fn ratchet_grab_bias(bias: f32, last_y: f32, now_y: f32) -> f32 {
    let down = now_y - last_y;
    if down <= 0.0 {
        return bias;
    }
    (bias - down * GRAB_BIAS_DRIFT_FRACTION).max(grab_bias_full_view().min(bias))
}

/// Which selection handle a lollipop represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HandleKind {
    /// The cursor handle shown for a collapsed selection: the caret stem with a
    /// round grab dot hanging below the line (like the end handle).
    Cursor,
    /// The start (leftmost) selection handle: dot ON TOP of the line, stem
    /// spanning the line box below it.
    SelectionStart,
    /// The end (rightmost) selection handle: stem spanning the line box, dot
    /// hanging BELOW it.
    SelectionEnd,
}

/// Radius of a selection/cursor handle dot in dp (the reference dot is
/// 16.2 physical px at 3x ≈ a 16 dp circle).
pub const HANDLE_RADIUS: f32 = 8.0;

/// Width of the handle stem in dp (measured 6 px at 3x = 2 dp — the same
/// weight as the caret).
pub const HANDLE_STEM_WIDTH: f32 = 2.0;

/// How far the dot dips INTO the line box (dp): the reference start dot's
/// bottom sits ~5 px (1.7 dp) below the line-box top, the end dot's top ~6 px
/// above the line-box bottom, so dot and stem read as one continuous shape.
pub const HANDLE_DOT_LINE_OVERLAP: f32 = 2.0;

/// SVG path data for a handle lollipop at a text edge.
///
/// `anchor_x` is the text edge (caret / selection endpoint) x; the line box
/// spans `line_top .. line_bottom`. The stem (width
/// [`HANDLE_STEM_WIDTH`]) always spans the line box, centered on `anchor_x`;
/// the dot (radius `radius`) sits tangent just outside the line box — above it
/// for [`SelectionStart`](HandleKind::SelectionStart), below it for
/// [`SelectionEnd`](HandleKind::SelectionEnd) and
/// [`Cursor`](HandleKind::Cursor) — overlapping the box edge by
/// [`HANDLE_DOT_LINE_OVERLAP`] so the two read as one shape.
pub fn handle_path_data(
    kind: HandleKind,
    anchor_x: f32,
    line_top: f32,
    line_bottom: f32,
    radius: f32,
) -> String {
    let r = radius.max(0.0);
    let half_stem = HANDLE_STEM_WIDTH * 0.5;
    let (left, right) = (anchor_x - half_stem, anchor_x + half_stem);
    let stem = |top: f32, bottom: f32| {
        format!("M {left} {top} L {right} {top} L {right} {bottom} L {left} {bottom} Z")
    };
    let dot = |cy: f32| {
        // Sweep flag 1 keeps the circle CLOCKWISE like the stem rectangle:
        // with the NonZero fill rule, same-direction subpaths union; opposite
        // windings cancel where dot and stem overlap, punching a notch at
        // the joint.
        format!(
            "M {x0} {cy} A {r} {r} 0 1 1 {x1} {cy} A {r} {r} 0 1 1 {x0} {cy} Z",
            x0 = anchor_x - r,
            x1 = anchor_x + r,
        )
    };
    match kind {
        HandleKind::SelectionStart => {
            // Dot on top: center a radius above the line top, minus the overlap.
            let cy = line_top - r + HANDLE_DOT_LINE_OVERLAP;
            format!("{} {}", stem(line_top, line_bottom), dot(cy))
        }
        HandleKind::SelectionEnd | HandleKind::Cursor => {
            // Dot below: center a radius under the line bottom, minus overlap.
            let cy = line_bottom + r - HANDLE_DOT_LINE_OVERLAP;
            format!("{} {}", stem(line_top, line_bottom), dot(cy))
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

    /// The tap-inside-selection ladder (bug c): a lone tap inside an existing
    /// selection grabs the word, and every further tap AT THE SAME SPOT grows
    /// the granularity word → line → paragraph, then cycles back to word — even
    /// when the taps arrive too slowly to count as a rapid multi-tap (the growth
    /// is keyed on location, not the double-tap timeout). Tapping a NEW spot
    /// resets to word.
    #[test]
    fn tap_inside_selection_cycles_word_line_paragraph_by_location() {
        use SelectionGranularity::*;

        // Start: a lone (slow) tap inside a selection. raw_tap_count == 1
        // (the timeout lapsed), but it still grabs the word.
        let mut count = resolve_selection_tap_count(1, 0, true, false);
        assert_eq!(count, 2);
        assert_eq!(tap_selection_granularity(count), Word);

        // Same spot again, still slow (raw == 1): grow to the line.
        count = resolve_selection_tap_count(1, count, true, true);
        assert_eq!(count, 3);
        assert_eq!(tap_selection_granularity(count), Line);

        // Same spot again: grow to the paragraph.
        count = resolve_selection_tap_count(1, count, true, true);
        assert_eq!(count, 4);
        assert_eq!(tap_selection_granularity(count), Paragraph);

        // Same spot again: cycle back to the word.
        count = resolve_selection_tap_count(1, count, true, true);
        assert_eq!(count, 5);
        assert_eq!(tap_selection_granularity(count), Word);

        // A tap at a NEW spot inside the selection resets to word.
        let reset = resolve_selection_tap_count(1, count, true, false);
        assert_eq!(reset, 2);
        assert_eq!(tap_selection_granularity(reset), Word);
    }

    /// A genuine rapid multi-tap keeps using its own running count, so
    /// [`resolve_selection_tap_count`] does not disturb the double→word,
    /// triple→line ladder, and a lone tap outside a selection stays a caret.
    #[test]
    fn resolve_tap_count_preserves_rapid_multitap_and_caret() {
        // Rapid multi-tap: pass the classify count straight through.
        assert_eq!(resolve_selection_tap_count(2, 1, false, false), 2);
        assert_eq!(resolve_selection_tap_count(3, 2, true, true), 3);
        // Lone tap outside any selection: caret (count 1).
        assert_eq!(resolve_selection_tap_count(1, 4, false, true), 1);
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
    fn handle_path_is_valid_and_spans_the_line_box() {
        let (x, top, bottom) = (40.0_f32, 20.0_f32, 40.0_f32);
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let data = handle_path_data(kind, x, top, bottom, HANDLE_RADIUS);
            let path = cranpose_ui_graphics::VectorPath::parse(&data)
                .expect("handle path must be valid SVG");
            assert!(!path.is_empty(), "{kind:?} handle must have geometry");
            let bounds = path.bounds();
            // The stem spans the line box, so the shape covers top..bottom.
            assert!(bounds.y <= top + 0.5, "{kind:?} must reach the line top");
            assert!(
                bounds.y + bounds.height >= bottom - 0.5,
                "{kind:?} must reach the line bottom"
            );
            // Horizontally centered on the anchor, a dot-radius each way.
            assert!((bounds.x - (x - HANDLE_RADIUS)).abs() <= 0.5);
            assert!((bounds.x + bounds.width - (x + HANDLE_RADIUS)).abs() <= 0.5);
        }
    }

    /// The reference lollipop orientation: the start handle's dot rides ON TOP
    /// of the line (center ~a radius above the line top), the end and cursor
    /// dots hang BELOW it, and every dot dips [`HANDLE_DOT_LINE_OVERLAP`] into
    /// the line box so dot + stem read as one continuous shape.
    #[test]
    fn selection_handle_dots_sit_on_the_correct_side_of_the_line() {
        let (x, top, bottom, r) = (40.0_f32, 20.0_f32, 40.0_f32, HANDLE_RADIUS);
        let eps = 0.5_f32;

        let bounds = |kind: HandleKind| {
            let data = handle_path_data(kind, x, top, bottom, r);
            cranpose_ui_graphics::VectorPath::parse(&data)
                .expect("valid handle path")
                .bounds()
        };

        // Start: the shape extends a dot-diameter ABOVE the line top (minus the
        // overlap), and not below the line bottom.
        let start = bounds(HandleKind::SelectionStart);
        assert!(
            (start.y - (top - 2.0 * r + HANDLE_DOT_LINE_OVERLAP)).abs() <= eps,
            "start dot must ride on top of the line (top at {}, expected {})",
            start.y,
            top - 2.0 * r + HANDLE_DOT_LINE_OVERLAP
        );
        assert!(
            start.y + start.height <= bottom + eps,
            "start handle must not extend below the line box"
        );

        // End and cursor: the shape extends a dot-diameter BELOW the line
        // bottom (minus the overlap), and not above the line top.
        for kind in [HandleKind::SelectionEnd, HandleKind::Cursor] {
            let b = bounds(kind);
            assert!(
                (b.y + b.height - (bottom + 2.0 * r - HANDLE_DOT_LINE_OVERLAP)).abs() <= eps,
                "{kind:?} dot must hang below the line (bottom at {}, expected {})",
                b.y + b.height,
                bottom + 2.0 * r - HANDLE_DOT_LINE_OVERLAP
            );
            assert!(
                b.y >= top - eps,
                "{kind:?} handle must not extend above the line box"
            );
        }
    }

    /// The wrap-aware caret line lookup (bug d): a caret on a wrapped line's
    /// later visual line must resolve to that visual line (not the logical
    /// line's first visual line), with the correct line-start byte so its x is
    /// measured from the start of the visual line.
    #[test]
    fn caret_visual_line_resolves_wrapped_visual_lines() {
        // "aaaa bbbb" wrapped into ["aaaa " (0..5), "bbbb" (5..9)], then a hard
        // newline to a short line "cc" (10..12).
        let ranges = vec![0..5usize, 5..9, 10..12];

        // Start of the first visual line.
        assert_eq!(
            caret_visual_line(&ranges, 0, LineAffinity::Downstream),
            (0, 0)
        );
        // Middle of the first visual line.
        assert_eq!(
            caret_visual_line(&ranges, 3, LineAffinity::Downstream),
            (0, 0)
        );
        // Start of the second (wrapped) visual line.
        assert_eq!(
            caret_visual_line(&ranges, 5, LineAffinity::Downstream),
            (1, 5)
        );
        // Middle of the second visual line — must NOT resolve to line 0.
        assert_eq!(
            caret_visual_line(&ranges, 7, LineAffinity::Downstream),
            (1, 5)
        );
        // End of the wrapped logical line.
        assert_eq!(
            caret_visual_line(&ranges, 9, LineAffinity::Downstream),
            (1, 5)
        );
        // The line after the hard newline.
        assert_eq!(
            caret_visual_line(&ranges, 11, LineAffinity::Downstream),
            (2, 10)
        );
        // End of text.
        assert_eq!(
            caret_visual_line(&ranges, 12, LineAffinity::Downstream),
            (2, 10)
        );
    }

    /// The grab-bias ratchet: downward finger travel migrates the handle
    /// above the finger at the documented fraction, saturating at full view;
    /// upward travel never un-migrates; a grab already deeper than the
    /// full-view offset holds its own floor.
    #[test]
    fn grab_bias_ratchet_migrates_down_only_to_full_view() {
        // Grab ON the line (finger at the stem): bias +8.
        let mut bias = 8.0;
        // Finger slides 20dp down: 35% absorbed.
        bias = ratchet_grab_bias(bias, 100.0, 120.0);
        assert!((bias - 1.0).abs() < 1e-4, "got {bias}");
        // Upward travel changes nothing.
        let up = ratchet_grab_bias(bias, 120.0, 90.0);
        assert_eq!(up, bias);
        // A long slide saturates at the full-view offset.
        let saturated = ratchet_grab_bias(bias, 90.0, 400.0);
        assert_eq!(saturated, grab_bias_full_view());
        // Once saturated, further downward travel holds.
        assert_eq!(
            ratchet_grab_bias(saturated, 400.0, 500.0),
            grab_bias_full_view()
        );
        // A grab deeper than full view (finger far below the dot) keeps its
        // own deeper bias instead of snapping up to the target.
        let deep = grab_bias_full_view() - 10.0;
        assert_eq!(ratchet_grab_bias(deep, 0.0, 50.0), deep);
    }

    #[test]
    fn caret_visual_line_handles_empty_ranges() {
        assert_eq!(caret_visual_line(&[], 5, LineAffinity::Upstream), (0, 0));
        assert_eq!(caret_visual_line(&[], 5, LineAffinity::Downstream), (0, 0));
    }

    /// A soft-wrap boundary byte is BOTH the end of the upper visual line and
    /// the start of the lower one. A finger dragging a selection END (or the
    /// caret/cursor handle, or the loupe) along the upper line's right edge
    /// produces exactly that byte — upstream affinity must keep the anchor on
    /// the upper line's end instead of snapping one line down to the left
    /// edge (the reported wrapped-multiline handle Y-offset bug).
    #[test]
    fn caret_visual_line_upstream_anchors_shared_wrap_boundary_to_upper_line() {
        let ranges = vec![0..5usize, 5..9, 10..12];

        // The shared boundary resolves per affinity.
        assert_eq!(
            caret_visual_line(&ranges, 5, LineAffinity::Upstream),
            (0, 0)
        );
        assert_eq!(
            caret_visual_line(&ranges, 5, LineAffinity::Downstream),
            (1, 5)
        );

        // Mid-line offsets are affinity-independent.
        assert_eq!(
            caret_visual_line(&ranges, 3, LineAffinity::Upstream),
            (0, 0)
        );
        assert_eq!(
            caret_visual_line(&ranges, 7, LineAffinity::Upstream),
            (1, 5)
        );

        // A hard-newline boundary is NOT shared (the ranges gap over the
        // separator): upstream must not pull the lower line's start up.
        assert_eq!(
            caret_visual_line(&ranges, 10, LineAffinity::Upstream),
            (2, 10)
        );

        // End of text stays on the last line under either affinity.
        assert_eq!(
            caret_visual_line(&ranges, 12, LineAffinity::Upstream),
            (2, 10)
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
