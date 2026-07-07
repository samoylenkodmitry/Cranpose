//! Draggable text-selection handles (finger teardrops) drawn in the top-level
//! overlay.
//!
//! A [`SelectionHandle`] renders one of the teardrop shapes from
//! [`crate::text_selection`] at a text endpoint (the caret, or a selection
//! start/end) and lets a finger drag it. It is composed inside a [`Popup`] so
//! it draws above the field and is not clipped when it hangs below the last
//! line. Positioning and drag→text-offset mapping are the caller's job (see
//! `BasicTextField`); this widget only draws the teardrop and reports the
//! window-space position of an in-progress drag.

#![allow(non_snake_case)]

use std::rc::Rc;

use crate::composable;
use crate::modifier::Modifier;
use crate::text_selection::{handle_path_data, HandleKind, HANDLE_GRAB_SLOP};
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

/// Geometry of a handle's drawable teardrop for a bulb `radius`: the box the
/// teardrop occupies (which doubles as its finger grab region) and where the tip
/// sits inside that box, so the caller can anchor the box at
/// `tip_endpoint - tip_in_box` to land the tip on the text.
struct HandleShape {
    /// SVG path of the teardrop in the box's local coordinates.
    path_data: String,
    /// The full box size including the finger grab slop. The box's own pointer
    /// input is the handle's grab region, so this must be finger-sized.
    box_size: Size,
    /// The tip position within the (padded) box.
    tip_in_box: Point,
}

/// Computes the teardrop path, box size and tip offset for a handle, expanding
/// the box by a finger-sized grab slop so the handle is easy to grab.
///
/// The box is the handle's grab region (its `Box` carries the drag pointer
/// input). A bare teardrop is far smaller than a fingertip, so without slop a
/// touch-DOWN aimed at the handle lands a few px off it and falls through to the
/// field below, which places a caret and collapses the selection. The slop is
/// applied symmetrically on the sides and BELOW the tip — never above it — so
/// the grab region stays generous for both the start and end handles yet keeps
/// off the glyph line above the tip.
fn handle_shape(kind: HandleKind, radius: f32) -> HandleShape {
    let slop = HANDLE_GRAB_SLOP.max(0.0);
    // Measure the teardrop with its tip at the origin to learn its bounds
    // (the bulb may extend left/right/below the tip depending on the kind).
    let bounds = VectorPath::parse(&handle_path_data(kind, 0.0, 0.0, radius))
        .map(|p| p.bounds())
        .unwrap_or(Rect {
            x: -radius,
            y: 0.0,
            width: 2.0 * radius,
            height: 2.0 * radius,
        });
    // Place the tip so the whole shape fits at non-negative coordinates inside
    // the box. Grab slop is added on the sides and BELOW the tip for a
    // finger-sized target on the bulb (which hangs below the line), but NOT
    // above the tip: the tip sits at the caret line's bottom, so any upward slop
    // would put the handle's grab region over the glyphs on that line. That
    // overlap is what made a double-tap regress after selection handles started
    // appearing inside `LazyColumn` items — the second tap landed on the newly
    // shown cursor handle (which moves the caret and consumes the event) instead
    // of reaching the field to escalate into a word selection. Keeping the grab
    // box strictly at/below the tip lets taps on the text through, so double-tap
    // word-select and long-press both work, while grabbing the handle (anywhere
    // within a finger's reach of the bulb below the line) now works reliably.
    //
    // Because the slop is the same on the left and the right of the drawn shape,
    // the start and end handles — whose teardrops are mirror images (the bulb
    // points opposite ways) — get grab regions that are mirror-symmetric about
    // their respective tips, so neither handle is harder to grab than the other.
    let tip_in_box = Point {
        x: slop - bounds.x,
        y: -bounds.y,
    };
    let box_size = Size {
        width: bounds.width + 2.0 * slop,
        height: bounds.height + slop,
    };
    let path_data = handle_path_data(kind, tip_in_box.x, tip_in_box.y, radius);
    HandleShape {
        path_data,
        box_size,
        tip_in_box,
    }
}

/// The window-space axis-aligned grab region for a handle whose tip sits at
/// `tip` (window coords). This is exactly the region covered by the handle's
/// `Box` pointer input, expressed in window coordinates, so a touch-DOWN within
/// it grabs the handle (the overlay `Popup` is hit-tested above the field, so a
/// grab always wins over the field's caret placement).
///
/// Exposed so the arbitration can be asserted in tests without a full
/// compose/layout/hit-test round-trip.
#[cfg(test)]
pub(crate) fn handle_grab_rect(kind: HandleKind, tip: Point, radius: f32) -> Rect {
    let shape = handle_shape(kind, radius);
    Rect {
        x: tip.x - shape.tip_in_box.x,
        y: tip.y - shape.tip_in_box.y,
        width: shape.box_size.width,
        height: shape.box_size.height,
    }
}

/// A finger-draggable selection/cursor handle rendered in the overlay.
///
/// * `kind` — which teardrop to draw (cursor caret, selection start/end).
/// * `tip` — window-space position of the text endpoint the tip points at
///   (typically the bottom of the line at the caret/selection edge).
/// * `radius` / `color` — bulb radius and fill.
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
    radius: f32,
    color: Color,
    on_drag: impl Fn(Point) + 'static,
    on_drag_end: impl Fn() + 'static,
    on_long_press: impl Fn() + 'static,
    on_tap: impl Fn() + 'static,
) {
    let shape = handle_shape(kind, radius);
    let anchor = Rect {
        x: tip.x - shape.tip_in_box.x,
        y: tip.y - shape.tip_in_box.y,
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

    /// Regression for the double-tap-to-select-word regression. The cursor
    /// handle — the one a single tap shows — must place its tip at the very top
    /// of its touch box, so the box does not overlap the glyphs on the caret's
    /// line. Otherwise a double-tap's second tap lands on the handle (which just
    /// moves the caret and consumes the event) instead of reaching the field to
    /// escalate into a word selection.
    #[test]
    fn cursor_handle_touch_box_sits_at_or_below_the_tip() {
        let shape = handle_shape(HandleKind::Cursor, HANDLE_RADIUS);
        // The box is anchored at `tip - tip_in_box`; `tip_in_box.y == 0` means
        // its top edge coincides with the tip (the caret line's bottom).
        assert!(
            shape.tip_in_box.y.abs() < 0.01,
            "cursor handle tip must sit at the top edge of its touch box \
             (tip_in_box.y = {}), so it never overlaps the text line above",
            shape.tip_in_box.y
        );
        // The bulb still hangs below the tip with room for a finger.
        assert!(
            shape.box_size.height >= 2.0 * HANDLE_RADIUS,
            "cursor handle box must extend below the tip so the bulb is grabbable"
        );
    }

    /// No handle keeps any grab slop *above* the tip: the box top is exactly the
    /// shape's own topmost point (no extra empty slop over the text line), while
    /// the side/below finger slop is retained.
    #[test]
    fn handles_have_no_padding_above_the_tip() {
        let slop = HANDLE_GRAB_SLOP.max(0.0);
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let shape = handle_shape(kind, HANDLE_RADIUS);
            let bounds = cranpose_ui_graphics::VectorPath::parse(&handle_path_data(
                kind,
                0.0,
                0.0,
                HANDLE_RADIUS,
            ))
            .map(|p| p.bounds())
            .expect("valid handle path");
            // Box top coincides with the shape's own topmost point: no upward
            // slop beyond the teardrop geometry itself.
            assert!(
                (shape.tip_in_box.y - (-bounds.y)).abs() < 0.01,
                "{kind:?}: expected no slop above the tip, tip_in_box.y = {} vs shape top {}",
                shape.tip_in_box.y,
                -bounds.y
            );
            // Side slop is applied for a finger-sized grab.
            assert!(
                shape.box_size.width >= bounds.width + 2.0 * slop - 0.01,
                "{kind:?}: side grab slop must be applied"
            );
        }
    }

    /// The grab region must be finger-sized: at least a fingertip across so a
    /// touch-DOWN aimed at the handle reliably lands inside it instead of
    /// falling through to the field (which would collapse the selection). A
    /// fingertip is ~24-32dp; the drawn teardrop alone (~2·radius) is far too
    /// small, so the box must add a generous slop.
    #[test]
    fn grab_region_is_finger_sized() {
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let rect = handle_grab_rect(kind, Point { x: 100.0, y: 100.0 }, HANDLE_RADIUS);
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

    /// Reproduces the reported start-vs-end asymmetry as a guard: the start and
    /// end selection handles point in opposite directions (their bulbs hang to
    /// opposite sides of the shared tip), so a bug that offset one hit-box the
    /// wrong way would make that handle harder to grab. Their grab regions must
    /// be mirror images **of each other** about the tip — the start box reaches
    /// as far LEFT of the tip as the end box reaches RIGHT (over the bulb), and
    /// both are the same size — so neither handle collapses the selection on a
    /// near-miss while the other drags fine.
    #[test]
    fn start_and_end_grab_regions_are_mirror_images() {
        let tip = Point { x: 100.0, y: 100.0 };
        let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS);
        let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS);

        // Same-size boxes.
        assert!(
            (start.width - end.width).abs() < 0.01 && (start.height - end.height).abs() < 0.01,
            "start {start:?} and end {end:?} grab boxes must be the same size"
        );

        let start_left = tip.x - start.x;
        let start_right = (start.x + start.width) - tip.x;
        let end_left = tip.x - end.x;
        let end_right = (end.x + end.width) - tip.x;

        // Each box extends toward its bulb (start left, end right) by the same
        // amount, and away from it by the same smaller amount — the boxes are
        // reflections of one another.
        assert!(
            (start_left - end_right).abs() < 0.01 && (start_right - end_left).abs() < 0.01,
            "start (l={start_left}, r={start_right}) and end (l={end_left}, r={end_right}) \
             grab boxes must be horizontal mirror images"
        );
        // Sanity: the box genuinely reaches over the bulb (a full diameter to the
        // handle's own side) so the bulb is grabbable.
        assert!(
            start_left >= 2.0 * HANDLE_RADIUS && end_right >= 2.0 * HANDLE_RADIUS,
            "each grab box must extend a full bulb-diameter toward its bulb"
        );
    }

    /// A touch-DOWN a finger's width to either side of, and below, a handle tip
    /// must still land inside the handle's grab region (so it grabs the handle),
    /// while a press a finger's reach up on the glyph line above the tip must NOT
    /// — that belongs to the field so double-tap word-select keeps working. This
    /// is the geometric heart of the fix: the overlay handle `Box` is hit-tested
    /// above the field, so any DOWN inside this region grabs the handle instead
    /// of collapsing the selection.
    #[test]
    fn grab_region_covers_a_fingers_reach_but_not_the_glyph_line() {
        let tip = Point { x: 100.0, y: 100.0 };
        let contains = |r: Rect, x: f32, y: f32| {
            x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
        };
        for kind in [
            HandleKind::Cursor,
            HandleKind::SelectionStart,
            HandleKind::SelectionEnd,
        ] {
            let r = handle_grab_rect(kind, tip, HANDLE_RADIUS);
            // A press ~a finger-half to the sides / below the tip grabs it.
            assert!(
                contains(r, tip.x - 16.0, tip.y + 12.0),
                "{kind:?}: a press below-left of the tip must grab the handle"
            );
            assert!(
                contains(r, tip.x + 16.0, tip.y + 12.0),
                "{kind:?}: a press below-right of the tip must grab the handle"
            );
            // A press a finger's reach ABOVE the tip (on the previous glyph
            // line) must miss: no grab slop is added above the tip, so it
            // reaches the field for caret placement / double-tap word-select.
            assert!(
                !contains(r, tip.x, tip.y - HANDLE_GRAB_SLOP),
                "{kind:?}: a press a finger's reach above the tip must not grab \
                 the handle (it belongs to the field)"
            );
        }
    }
}
