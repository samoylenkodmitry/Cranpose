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
            previous_count.max(1).saturating_add(1)
        } else {
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
    let start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
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
    let start = text[..pos].rfind("\n\n").map_or(0, |i| {
        let mut s = i + 1;
        while text[s..].starts_with('\n') {
            s += 1;
        }
        s
    });
    let end = text[pos..].find("\n\n").map_or(text.len(), |i| pos + i);
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

/// Downward travel that follows with the original finger-to-handle offset
/// before the visibility drift starts.
pub const GRAB_DIRECT_FOLLOW_DISTANCE: f32 = 8.0;
/// Additional downward travel over which the handle moves into full view.
pub const GRAB_VISIBILITY_DRIFT_DISTANCE: f32 = 48.0;
/// Extra clearance (dp) below the handle dot once fully visible above the
/// finger.
pub const GRAB_BIAS_VIEW_CLEARANCE: f32 = 4.0;

/// The drift target: bias placing the finger just below the handle dot
/// (tip + dot + clearance), so the whole lollipop stays visible above it.
pub fn grab_bias_full_view() -> f32 {
    -(2.0 * HANDLE_RADIUS + GRAB_BIAS_VIEW_CLEARANCE)
}

/// Finger-to-handle relationship for one drag. The first phase preserves the
/// captured offset exactly, the second shifts the handle above the finger,
/// and the third preserves that final offset exactly. Progress is based on
/// the furthest displacement from the grab, so event cadence and small
/// reversals cannot change the result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandleGrabOffset {
    initial_bias: f32,
    bias: f32,
    start_y: f32,
    furthest_y: f32,
    drift_progress: f32,
    drifts: bool,
}

impl HandleGrabOffset {
    pub fn begin(handle_tip_y: f32, finger_y: f32) -> Self {
        Self::begin_for(handle_tip_y, finger_y, true)
    }

    pub fn begin_for(handle_tip_y: f32, finger_y: f32, drifts: bool) -> Self {
        let initial_bias = handle_tip_y - finger_y;
        Self {
            initial_bias,
            bias: initial_bias,
            start_y: finger_y,
            furthest_y: finger_y,
            drift_progress: 0.0,
            drifts,
        }
    }

    pub fn track(&mut self, finger_y: f32) -> f32 {
        if !self.drifts {
            self.bias = self.initial_bias;
            return self.bias;
        }
        self.furthest_y = self.furthest_y.max(finger_y);
        let travel = (self.furthest_y - self.start_y - GRAB_DIRECT_FOLLOW_DISTANCE).max(0.0);
        let t = (travel / GRAB_VISIBILITY_DRIFT_DISTANCE).clamp(0.0, 1.0);
        self.drift_progress = t * t * (3.0 - 2.0 * t);
        let full_view = self.initial_bias.min(grab_bias_full_view());
        self.bias = self.initial_bias + (full_view - self.initial_bias) * self.drift_progress;
        self.bias
    }

    pub fn bias(&self) -> f32 {
        self.bias
    }

    pub fn drift_progress(&self) -> f32 {
        self.drift_progress
    }
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
        format!(
            "M {x0} {cy} A {r} {r} 0 1 1 {x1} {cy} A {r} {r} 0 1 1 {x0} {cy} Z",
            x0 = anchor_x - r,
            x1 = anchor_x + r,
        )
    };
    match kind {
        HandleKind::SelectionStart => {
            let cy = line_top - r + HANDLE_DOT_LINE_OVERLAP;
            format!("{} {}", stem(line_top, line_bottom), dot(cy))
        }
        HandleKind::SelectionEnd | HandleKind::Cursor => {
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
        HandleKind::Cursor => (dragged_offset, dragged_offset),
    }
}

#[cfg(test)]
#[path = "tests/text_selection_tests.rs"]
mod tests;
