//! Draggable text-selection handles (accent lollipops) drawn in the top-level
//! overlay.
//!
//! A [`SelectionHandle`] renders one of the lollipop shapes from
//! [`crate::text_selection`] at a text endpoint (the caret, or a selection
//! start/end) and lets a finger drag it: a 2 dp stem spanning the line box
//! with a 16 dp dot tangent just outside it — above the line for the start
//! handle, below it for the end and cursor handles. It is composed inside a
//! [`Popup`] so it draws above the field and is not clipped when it hangs
//! outside the line. Positioning and drag→text-offset mapping are the
//! caller's job (see `BasicTextField`); this widget only draws the lollipop
//! and reports the window-space position of an in-progress drag.

#![allow(non_snake_case)]

use std::rc::Rc;

use crate::composable;
use crate::modifier::Modifier;
use crate::text_selection::{
    handle_path_data, HandleKind, HANDLE_DOT_LINE_OVERLAP, HANDLE_GRAB_SLOP, HANDLE_STEM_WIDTH,
};
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::popup::Popup;
use crate::PointerInputScope;
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Point, Rect, Size, VectorPath};

/// How long (ms) the finger must rest on a handle — without moving beyond
/// [`HANDLE_LONG_PRESS_SLOP_PX`] — before it counts as a long-press. Matches
/// Android's ~500ms long-press timeout.
pub(crate) const HANDLE_LONG_PRESS_TIMEOUT_MS: i64 = 500;
/// Movement (px) tolerated during a long-press before it is treated as a drag
/// instead. Keeps a resting finger a long-press even with minor jitter.
pub(crate) const HANDLE_LONG_PRESS_SLOP_PX: f32 = 12.0;
/// Movement (px) tolerated between a handle's press and release for the gesture
/// to count as a tap (a quick press→release that did not drag the handle). A tap
/// on the collapsed cursor handle opens its action popup (Paste / Select all /
/// Undo / Redo).
pub(crate) const HANDLE_TAP_SLOP_PX: f32 = 12.0;

/// Geometry of a handle's drawable lollipop: the box it occupies (which
/// doubles as its finger grab region) and where the anchor — the text edge at
/// the line BOTTOM — sits inside that box, so the caller can anchor the box at
/// `tip_endpoint - tip_in_box` to land the stem on the text edge.
struct HandleShape {
    /// SVG path of the lollipop in the box's local coordinates.
    path_data: String,
    /// The full box size including the finger grab slop. The box's own pointer
    /// input is the handle's grab region, so this must be finger-sized.
    box_size: Size,
    /// The anchor position (text edge at the line bottom) within the box.
    tip_in_box: Point,
}

/// Computes the lollipop path, box size and anchor offset for a handle,
/// expanding the box by a finger-sized grab slop so the handle is easy to
/// grab. `line_height` is the line box the stem spans, ending at the anchor
/// (the line bottom).
///
/// The box is the handle's grab region (its `Box` carries the drag pointer
/// input) and the slop rules are per kind:
///
/// * the CURSOR handle covers only its dot below the line (sides/below slop,
///   nothing above the line bottom). Its stem is the field's own caret, and
///   any grab region over the glyph line would swallow the second tap of a
///   double-tap (which must reach the field to escalate into a word
///   selection) — the exact regression this shape used to pin as a teardrop;
/// * the START handle covers its dot above the line (slop above/sides) plus
///   the stem column across the line box, with no slop below the line bottom
///   (the next line's glyphs belong to the field);
/// * the END handle mirrors it: the stem column from the line top (no slop
///   above — the line's own glyphs stay tappable) down over its dot with
///   slop below.
///
/// Grabbing an edge ON the line (the stem column) is what the reference does:
/// a drag starting there rides the handle with the loupe up; a drag on the
/// dot below drags without the loupe.
fn handle_shape(kind: HandleKind, radius: f32, line_height: f32) -> HandleShape {
    let slop = HANDLE_GRAB_SLOP.max(0.0);
    let line_height = line_height.max(1.0);
    let half_width = radius.max(HANDLE_STEM_WIDTH * 0.5);
    // Vertical extent of the drawable relative to the anchor (line bottom).
    let (draw_top, draw_bottom) = match kind {
        // Dot above the line top (dipping HANDLE_DOT_LINE_OVERLAP into it).
        HandleKind::SelectionStart => (-line_height - 2.0 * radius + HANDLE_DOT_LINE_OVERLAP, 0.0),
        // Stem across the line, dot hanging below.
        HandleKind::SelectionEnd => (-line_height, 2.0 * radius - HANDLE_DOT_LINE_OVERLAP),
        // Dot below the line only (the field's caret is the stem).
        HandleKind::Cursor => (0.0, 2.0 * radius - HANDLE_DOT_LINE_OVERLAP),
    };
    // Grab slop: sides always; above only where the handle's dot is above
    // (start), below only where it hangs below (end/cursor).
    let (slop_above, slop_below) = match kind {
        HandleKind::SelectionStart => (slop, 0.0),
        HandleKind::SelectionEnd | HandleKind::Cursor => (0.0, slop),
    };
    let box_top = draw_top - slop_above;
    let box_bottom = draw_bottom + slop_below;
    let tip_in_box = Point {
        x: half_width + slop,
        y: -box_top,
    };
    let box_size = Size {
        width: 2.0 * half_width + 2.0 * slop,
        height: box_bottom - box_top,
    };
    // The path in box-local coordinates. The cursor handle draws only its dot
    // (its stem is the field's caret); start/end draw stem + dot.
    let (line_top_local, line_bottom_local) = (tip_in_box.y - line_height, tip_in_box.y);
    let path_data = if kind == HandleKind::Cursor {
        // Dot tangent below the line bottom, overlap folded in.
        let cy = line_bottom_local + radius - HANDLE_DOT_LINE_OVERLAP;
        format!(
            "M {x0} {cy} A {r} {r} 0 1 1 {x1} {cy} A {r} {r} 0 1 1 {x0} {cy} Z",
            x0 = tip_in_box.x - radius,
            x1 = tip_in_box.x + radius,
            r = radius,
        )
    } else {
        handle_path_data(
            kind,
            tip_in_box.x,
            line_top_local,
            line_bottom_local,
            radius,
        )
    };
    HandleShape {
        path_data,
        box_size,
        tip_in_box,
    }
}

/// The window-space axis-aligned grab region for a handle whose anchor (text
/// edge at the line bottom) sits at `tip` (window coords). This is exactly the
/// region covered by the handle's `Box` pointer input, expressed in window
/// coordinates, so a touch-DOWN within it grabs the handle (the overlay
/// `Popup` is hit-tested above the field, so a grab always wins over the
/// field's caret placement).
///
/// Exposed so the arbitration can be asserted in tests without a full
/// compose/layout/hit-test round-trip.
#[cfg(test)]
pub(crate) fn handle_grab_rect(
    kind: HandleKind,
    tip: Point,
    radius: f32,
    line_height: f32,
) -> Rect {
    let shape = handle_shape(kind, radius, line_height);
    Rect {
        x: tip.x - shape.tip_in_box.x,
        y: tip.y - shape.tip_in_box.y,
        width: shape.box_size.width,
        height: shape.box_size.height,
    }
}

/// A finger-draggable selection/cursor handle rendered in the overlay.
///
/// * `kind` — which lollipop to draw (cursor dot, selection start/end).
/// * `tip` — window-space position of the text edge at the line BOTTOM (the
///   caret/selection endpoint).
/// * `line_height` — the line box the stem spans (up from `tip`).
/// * `radius` / `color` — dot radius and accent fill.
/// * `on_drag` — invoked with the current drag position (window space) on every
///   pointer down/move so the field can map it to a text offset and move the
///   caret / extend the selection.
/// * `on_drag_end` — invoked when the finger lifts, so the field can settle
///   (e.g. show the contextual menu).
/// * `on_long_press` — invoked once when the finger rests on the handle past the
///   long-press timeout without dragging it, so the field can (re)open the
///   contextual menu even when the selection range has not changed.
/// * `on_tap` — invoked when the finger lifts after a quick press that did not
///   drag the handle beyond [`HANDLE_TAP_SLOP_PX`] (and was not a long-press),
///   so the collapsed cursor handle can open its action popup.
#[allow(clippy::too_many_arguments)]
#[composable]
pub fn SelectionHandle(
    kind: HandleKind,
    tip: Point,
    line_height: f32,
    radius: f32,
    color: Color,
    on_drag: impl Fn(Point) + 'static,
    on_drag_end: impl Fn() + 'static,
    on_long_press: impl Fn() + 'static,
    on_tap: impl Fn() + 'static,
) {
    let shape = handle_shape(kind, radius, line_height);
    // Snap the box to whole logical pixels: the stem is a 2dp bar whose
    // box-local coordinates are integral, so a fractional anchor gives one
    // handle a 7px stem and the other a 6px one at 3x (anti-aliased
    // asymmetry the reference never shows).
    let anchor = Rect {
        x: (tip.x - shape.tip_in_box.x).round(),
        y: (tip.y - shape.tip_in_box.y).round(),
        width: 0.0,
        height: 0.0,
    };
    let path_data = shape.path_data;
    let box_size = shape.box_size;
    let on_drag: Rc<dyn Fn(Point)> = Rc::new(on_drag);
    let on_drag_end: Rc<dyn Fn()> = Rc::new(on_drag_end);
    let on_long_press: Rc<dyn Fn()> = Rc::new(on_long_press);
    let on_tap: Rc<dyn Fn()> = Rc::new(on_tap);

    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let path_data = path_data.clone();
        let on_drag = Rc::clone(&on_drag);
        let on_drag_end = Rc::clone(&on_drag_end);
        let on_long_press = Rc::clone(&on_long_press);
        let on_tap = Rc::clone(&on_tap);
        Box(
            Modifier::empty()
                .size(box_size)
                .draw_behind(move |scope: &mut dyn DrawScope| {
                    if let Ok(path) = VectorPath::parse(&path_data) {
                        scope.draw_vector_path(&path, Brush::solid(color));
                    }
                })
                .then(selection_handle_pointer_input(
                    kind,
                    Rc::clone(&on_drag),
                    Rc::clone(&on_drag_end),
                    Rc::clone(&on_long_press),
                    Rc::clone(&on_tap),
                )),
            BoxSpec::default(),
            || {},
        );
    });
}

/// Builds the pointer-input modifier that drives a selection handle: it reports
/// every drag position, the drag end (finger lift), and a long-press (finger
/// held in place past [`HANDLE_LONG_PRESS_TIMEOUT_MS`]). Shared by
/// [`SelectionHandle`] and exercised directly in tests so the gesture semantics
/// stay pinned without a full compose/layout/hit-test round-trip.
///
/// The gesture task is keyed by the handle `kind`. This is load-bearing: when a
/// field's selection collapses to a caret and then re-expands into a range, the
/// composition reuses the single cursor handle's positional slot for the range's
/// **start** handle. A constant key would keep the running gesture task (and the
/// `on_drag` closure it captured) from the previous frame, so the start handle
/// would still execute the *cursor* handle's caret-placement drag and COLLAPSE
/// the selection on grab — while the end handle (a fresh slot) worked fine, the
/// exact start-vs-end asymmetry users hit. Keying by `kind` restarts the task
/// with the correct `on_drag` when the slot's kind changes.
pub(crate) fn selection_handle_pointer_input(
    kind: HandleKind,
    on_drag: Rc<dyn Fn(Point)>,
    on_drag_end: Rc<dyn Fn()>,
    on_long_press: Rc<dyn Fn()>,
    on_tap: Rc<dyn Fn()>,
) -> Modifier {
    Modifier::empty().pointer_input(kind, move |scope: PointerInputScope| {
        let on_drag = Rc::clone(&on_drag);
        let on_drag_end = Rc::clone(&on_drag_end);
        let on_long_press = Rc::clone(&on_long_press);
        let on_tap = Rc::clone(&on_tap);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    // Track the initial press so a resting finger can be told
                    // apart from a drag. `time_ms` is the platform input clock;
                    // when it is unavailable the long-press simply never fires
                    // (a drag/lift still settles the selection).
                    let mut down_time: Option<i64> = None;
                    let mut down_pos = Point { x: 0.0, y: 0.0 };
                    let mut long_press_fired = false;
                    // Whether the finger has strayed beyond the tap slop since it
                    // went down — a stray means a drag, not a tap.
                    let mut dragged = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                down_time = event.time_ms;
                                down_pos = event.global_position;
                                long_press_fired = false;
                                dragged = false;
                                on_drag(event.global_position);
                                event.consume();
                            }
                            PointerEventKind::Move => {
                                if moved_beyond(down_pos, event.global_position, HANDLE_TAP_SLOP_PX)
                                {
                                    dragged = true;
                                }
                                on_drag(event.global_position);
                                maybe_fire_long_press(
                                    &event,
                                    down_time,
                                    down_pos,
                                    &mut long_press_fired,
                                    &on_long_press,
                                );
                                event.consume();
                            }
                            PointerEventKind::Up => {
                                maybe_fire_long_press(
                                    &event,
                                    down_time,
                                    down_pos,
                                    &mut long_press_fired,
                                    &on_long_press,
                                );
                                if moved_beyond(down_pos, event.global_position, HANDLE_TAP_SLOP_PX)
                                {
                                    dragged = true;
                                }
                                on_drag_end();
                                // A quick press→release that neither dragged the
                                // handle nor became a long-press is a tap: open
                                // the handle's action popup.
                                if !dragged && !long_press_fired {
                                    on_tap();
                                }
                                down_time = None;
                                event.consume();
                            }
                            PointerEventKind::Cancel => {
                                on_drag_end();
                                down_time = None;
                                event.consume();
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

/// Whether `now` is more than `slop` px from `origin` (used to tell a tap from a
/// drag on a selection handle).
fn moved_beyond(origin: Point, now: Point, slop: f32) -> bool {
    let dx = now.x - origin.x;
    let dy = now.y - origin.y;
    dx * dx + dy * dy > slop * slop
}

/// Fires `on_long_press` at most once per press when the finger has rested on
/// the handle for at least [`HANDLE_LONG_PRESS_TIMEOUT_MS`] without moving more
/// than [`HANDLE_LONG_PRESS_SLOP_PX`] from where it went down (i.e. a hold, not
/// a drag).
fn maybe_fire_long_press(
    event: &PointerEvent,
    down_time: Option<i64>,
    down_pos: Point,
    long_press_fired: &mut bool,
    on_long_press: &Rc<dyn Fn()>,
) {
    if *long_press_fired {
        return;
    }
    let (Some(down_ms), Some(now_ms)) = (down_time, event.time_ms) else {
        return;
    };
    if is_handle_long_press(down_ms, now_ms, down_pos, event.global_position) {
        *long_press_fired = true;
        on_long_press();
    }
}

/// Pure predicate for a handle long-press: held long enough and moved little
/// enough to be a rest rather than a drag.
pub(crate) fn is_handle_long_press(
    down_ms: i64,
    now_ms: i64,
    down_pos: Point,
    now_pos: Point,
) -> bool {
    let dx = now_pos.x - down_pos.x;
    let dy = now_pos.y - down_pos.y;
    let moved = (dx * dx + dy * dy).sqrt();
    now_ms - down_ms >= HANDLE_LONG_PRESS_TIMEOUT_MS && moved <= HANDLE_LONG_PRESS_SLOP_PX
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_selection::HANDLE_RADIUS;

    const LINE_HEIGHT: f32 = 20.0;

    /// Regression guard for double-tap-to-select-word. The cursor handle — the
    /// one a single tap shows — must keep its whole touch box AT OR BELOW the
    /// line bottom (its dot hangs under the line; its stem is the field's own
    /// caret). Any grab region over the glyph line would swallow the second
    /// tap of a double-tap (which must reach the field to escalate into a word
    /// selection).
    #[test]
    fn cursor_handle_touch_box_sits_at_or_below_the_line_bottom() {
        let shape = handle_shape(HandleKind::Cursor, HANDLE_RADIUS, LINE_HEIGHT);
        // The box is anchored at `tip - tip_in_box`; `tip_in_box.y == 0` means
        // its top edge coincides with the anchor (the caret line's bottom).
        assert!(
            shape.tip_in_box.y.abs() < 0.01,
            "cursor handle anchor must sit at the top edge of its touch box \
             (tip_in_box.y = {}), so it never overlaps the text line above",
            shape.tip_in_box.y
        );
        // The dot still hangs below the anchor with room for a finger.
        assert!(
            shape.box_size.height >= 2.0 * HANDLE_RADIUS,
            "cursor handle box must extend below the anchor so the dot is grabbable"
        );
        // And it draws ONLY the dot — the stem is the field's caret, which
        // keeps blinking; a drawn stem would paint over it.
        assert!(
            !shape.path_data.contains('L'),
            "cursor handle must draw only its dot (no stem rectangle): {}",
            shape.path_data
        );
    }

    /// The grab region must be finger-sized: at least a fingertip across so a
    /// touch-DOWN aimed at the handle reliably lands inside it instead of
    /// falling through to the field (which would collapse the selection). A
    /// fingertip is ~24-32dp; the drawn dot alone (~2·radius) is far too
    /// small, so the box must add a generous slop.
    #[test]
    fn grab_region_is_finger_sized() {
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let rect = handle_grab_rect(
                kind,
                Point { x: 100.0, y: 100.0 },
                HANDLE_RADIUS,
                LINE_HEIGHT,
            );
            assert!(
                rect.width >= 48.0,
                "{kind:?}: grab region must be at least a fingertip wide, got {}",
                rect.width
            );
            assert!(
                rect.height >= 32.0,
                "{kind:?}: grab region must be at least a fingertip tall, got {}",
                rect.height
            );
        }
    }

    /// The start and end grab regions are vertical mirror images of each other
    /// about the line box: the start covers its dot ABOVE the line (plus the
    /// stem column across the line), the end covers the stem column and its dot
    /// BELOW — so neither handle is harder to grab than the other.
    #[test]
    fn start_and_end_grab_regions_mirror_about_the_line() {
        let tip = Point { x: 100.0, y: 100.0 };
        let line_top = tip.y - LINE_HEIGHT;
        let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS, LINE_HEIGHT);
        let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS, LINE_HEIGHT);

        // Same-size boxes.
        assert!(
            (start.width - end.width).abs() < 0.01 && (start.height - end.height).abs() < 0.01,
            "start {start:?} and end {end:?} grab boxes must be the same size"
        );
        // Start reaches above the line top as far as end reaches below the
        // line bottom (dot + slop), and each stops at the opposite line edge.
        let start_above = line_top - start.y;
        let end_below = (end.y + end.height) - tip.y;
        assert!(
            (start_above - end_below).abs() < 0.01,
            "start reach above the line ({start_above}) must equal end reach below ({end_below})"
        );
        assert!(
            ((start.y + start.height) - tip.y).abs() < 0.01,
            "start grab box must stop at the line bottom (no slop below)"
        );
        assert!(
            (end.y - line_top).abs() < 0.01,
            "end grab box must start at the line top (no slop above)"
        );
    }

    /// Per-kind grab arbitration: a press on a handle's dot (or a finger-width
    /// beside it) grabs the handle; a press one line AWAY from the handle's
    /// extent — the neighbouring glyph line — must fall through to the field
    /// so taps/double-taps there keep working. Start and end handles also
    /// cover their stem column ON the line (the reference grabs an edge on the
    /// line and rides it with the loupe up); the cursor handle never covers
    /// the line (double-tap protection).
    #[test]
    fn grab_regions_cover_the_dot_and_stem_but_not_neighbouring_lines() {
        let tip = Point { x: 100.0, y: 100.0 };
        let line_top = tip.y - LINE_HEIGHT;
        let contains = |r: Rect, x: f32, y: f32| {
            x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
        };

        // Cursor: dot below the line is grabbable; the line above is not.
        let cursor = handle_grab_rect(HandleKind::Cursor, tip, HANDLE_RADIUS, LINE_HEIGHT);
        assert!(contains(cursor, tip.x - 16.0, tip.y + 12.0));
        assert!(contains(cursor, tip.x + 16.0, tip.y + 12.0));
        assert!(
            !contains(cursor, tip.x, tip.y - 2.0),
            "cursor: a press on the glyph line belongs to the field"
        );

        // Start: dot above the line and the stem column on the line grab it;
        // the line BELOW (next line's glyphs) does not.
        let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS, LINE_HEIGHT);
        assert!(
            contains(start, tip.x, line_top - HANDLE_RADIUS),
            "start: a press on the dot above the line must grab the handle"
        );
        assert!(
            contains(start, tip.x, tip.y - LINE_HEIGHT * 0.5),
            "start: a press on the stem column (on the line) must grab the handle"
        );
        assert!(
            !contains(start, tip.x, tip.y + 4.0),
            "start: a press below the line belongs to the field"
        );

        // End: stem column and dot below grab it; the line ABOVE does not.
        let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS, LINE_HEIGHT);
        assert!(
            contains(end, tip.x, tip.y + HANDLE_RADIUS),
            "end: a press on the dot below the line must grab the handle"
        );
        assert!(
            contains(end, tip.x, tip.y - LINE_HEIGHT * 0.5),
            "end: a press on the stem column (on the line) must grab the handle"
        );
        assert!(
            !contains(end, tip.x, line_top - 4.0),
            "end: a press above the line belongs to the field"
        );
    }
}
