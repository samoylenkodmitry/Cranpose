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

use std::{cell::Cell, rc::Rc};

use cranpose_core::remember;
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::{Brush, Color, DrawScope, Point, Rect, Size, VectorPath};

use crate::{
    PointerInputScope, composable,
    modifier::Modifier,
    text_selection::{
        HANDLE_DOT_LINE_OVERLAP, HANDLE_GRAB_SLOP, HANDLE_STEM_WIDTH, HandleKind, handle_path_data,
    },
    widgets::{
        box_widget::{Box, BoxSpec},
        popup::Popup,
    },
};

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
    path_data: String,
    box_size: Size,
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
    let (draw_top, draw_bottom) = match kind {
        HandleKind::SelectionStart => (-line_height - 2.0 * radius + HANDLE_DOT_LINE_OVERLAP, 0.0),
        HandleKind::SelectionEnd => (-line_height, 2.0 * radius - HANDLE_DOT_LINE_OVERLAP),
        HandleKind::Cursor => (0.0, 2.0 * radius - HANDLE_DOT_LINE_OVERLAP),
    };
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
    let (line_top_local, line_bottom_local) = (tip_in_box.y - line_height, tip_in_box.y);
    let path_data = if kind == HandleKind::Cursor {
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

/// Draw-phase glide state for a handle: position/velocity of the drawn
/// lollipop trailing its true anchor, integrated on real time per redraw.
#[derive(Clone, Copy)]
struct GlideState {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    last_nanos: u64,
}

fn glide_clock_nanos() -> u64 {
    use std::sync::OnceLock;

    use web_time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
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
///   drag the handle beyond `HANDLE_TAP_SLOP_PX` (and was not a long-press),
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
    let glide: Rc<Cell<GlideState>> = remember(|| {
        Rc::new(Cell::new(GlideState {
            x: f32::NAN,
            y: f32::NAN,
            vx: 0.0,
            vy: 0.0,
            last_nanos: 0,
        }))
    })
    .with(Rc::clone);
    {
        let state = glide.get();
        let snap_distance = (line_height * 1.5).max(24.0);
        let jump = ((tip.x - state.x).powi(2) + (tip.y - state.y).powi(2)).sqrt();
        if !state.x.is_finite() || !state.y.is_finite() || jump > snap_distance {
            glide.set(GlideState {
                x: tip.x,
                y: tip.y,
                vx: 0.0,
                vy: 0.0,
                last_nanos: 0,
            });
        }
    }
    let anchor = Rect {
        x: (tip.x - shape.tip_in_box.x).round(),
        y: (tip.y - shape.tip_in_box.y).round(),
        width: 0.0,
        height: 0.0,
    };
    let glide_tip = tip;
    let glide_for_draw = Rc::clone(&glide);
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
        let glide_for_draw = Rc::clone(&glide_for_draw);
        Box(
            Modifier::empty()
                .size(box_size)
                .draw_behind(move |scope: &mut dyn DrawScope| {
                    if kind == HandleKind::Cursor {
                        if let Ok(path) = VectorPath::parse(&path_data) {
                            scope.draw_vector_path(&path, Brush::solid(color));
                        }
                        return;
                    }
                    let mut state = glide_for_draw.get();
                    let settled = state.x.is_finite()
                        && (state.x - glide_tip.x).abs() < 0.25
                        && (state.y - glide_tip.y).abs() < 0.25
                        && state.vx.abs() < 2.0
                        && state.vy.abs() < 2.0;
                    if settled {
                        if state.last_nanos != 0 {
                            state = GlideState {
                                x: glide_tip.x,
                                y: glide_tip.y,
                                vx: 0.0,
                                vy: 0.0,
                                last_nanos: 0,
                            };
                            glide_for_draw.set(state);
                        }
                    } else {
                        let now_nanos = glide_clock_nanos();
                        let dt = if state.last_nanos == 0 {
                            0.0
                        } else {
                            ((now_nanos - state.last_nanos) as f32 / 1.0e9).min(0.05)
                        };
                        state.last_nanos = now_nanos;
                        if dt > 0.0 && state.x.is_finite() {
                            let omega = 40.0f32;
                            let decay = (-omega * dt).exp();
                            let sx = state.x - glide_tip.x;
                            let sy = state.y - glide_tip.y;
                            let cx = state.vx + omega * sx;
                            let cy = state.vy + omega * sy;
                            state.x = glide_tip.x + (sx + cx * dt) * decay;
                            state.y = glide_tip.y + (sy + cy * dt) * decay;
                            state.vx = (state.vx - omega * cx * dt) * decay;
                            state.vy = (state.vy - omega * cy * dt) * decay;
                        }
                        glide_for_draw.set(state);
                        crate::request_current_draw_redraw();
                    }
                    let (dx, dy) = if state.x.is_finite() {
                        (state.x - glide_tip.x, state.y - glide_tip.y)
                    } else {
                        (0.0, 0.0)
                    };
                    if let Ok(path) = VectorPath::parse(&path_data) {
                        scope.draw_vector_path(&path.translated(dx, dy), Brush::solid(color));
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
                    let mut down_time: Option<i64> = None;
                    let mut down_pos = Point { x: 0.0, y: 0.0 };
                    let mut pressed = false;
                    let mut long_press_fired = false;
                    let mut dragged = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                down_time = event.time_ms;
                                down_pos = event.global_position;
                                pressed = true;
                                long_press_fired = false;
                                dragged = false;
                                on_drag(event.global_position);
                                event.consume();
                            }
                            PointerEventKind::Move => {
                                if !pressed {
                                    continue;
                                }
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
                                if !pressed {
                                    continue;
                                }
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
                                pressed = false;
                                on_drag_end();
                                if !dragged && !long_press_fired {
                                    on_tap();
                                }
                                down_time = None;
                                event.consume();
                            }
                            PointerEventKind::Cancel => {
                                if pressed {
                                    pressed = false;
                                    on_drag_end();
                                }
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

    #[test]
    fn cursor_handle_touch_box_sits_at_or_below_the_line_bottom() {
        let shape = handle_shape(HandleKind::Cursor, HANDLE_RADIUS, LINE_HEIGHT);
        assert!(
            shape.tip_in_box.y.abs() < 0.01,
            "cursor handle anchor must sit at the top edge of its touch box \
             (tip_in_box.y = {}), so it never overlaps the text line above",
            shape.tip_in_box.y
        );
        assert!(
            shape.box_size.height >= 2.0 * HANDLE_RADIUS,
            "cursor handle box must extend below the anchor so the dot is grabbable"
        );
        assert!(
            !shape.path_data.contains('L'),
            "cursor handle must draw only its dot (no stem rectangle): {}",
            shape.path_data
        );
    }

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

    #[test]
    fn start_and_end_grab_regions_mirror_about_the_line() {
        let tip = Point { x: 100.0, y: 100.0 };
        let line_top = tip.y - LINE_HEIGHT;
        let start = handle_grab_rect(HandleKind::SelectionStart, tip, HANDLE_RADIUS, LINE_HEIGHT);
        let end = handle_grab_rect(HandleKind::SelectionEnd, tip, HANDLE_RADIUS, LINE_HEIGHT);

        assert!(
            (start.width - end.width).abs() < 0.01 && (start.height - end.height).abs() < 0.01,
            "start {start:?} and end {end:?} grab boxes must be the same size"
        );
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

    #[test]
    fn grab_regions_cover_the_dot_and_stem_but_not_neighbouring_lines() {
        let tip = Point { x: 100.0, y: 100.0 };
        let line_top = tip.y - LINE_HEIGHT;
        let contains = |r: Rect, x: f32, y: f32| {
            x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
        };

        let cursor = handle_grab_rect(HandleKind::Cursor, tip, HANDLE_RADIUS, LINE_HEIGHT);
        assert!(contains(cursor, tip.x - 16.0, tip.y + 12.0));
        assert!(contains(cursor, tip.x + 16.0, tip.y + 12.0));
        assert!(
            !contains(cursor, tip.x, tip.y - 2.0),
            "cursor: a press on the glyph line belongs to the field"
        );

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
