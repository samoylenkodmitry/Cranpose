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
use crate::text_selection::{handle_path_data, HandleKind, HANDLE_TOUCH_PADDING};
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

/// Geometry of a handle's drawable teardrop for a bulb `radius`: the box the
/// teardrop occupies and where the tip sits inside that box, so the caller can
/// anchor the box at `tip_endpoint - tip_in_box` to land the tip on the text.
struct HandleShape {
    /// SVG path of the teardrop in the box's local coordinates.
    path_data: String,
    /// The full box size including the finger touch padding.
    box_size: Size,
    /// The tip position within the (padded) box.
    tip_in_box: Point,
}

/// Computes the teardrop path, box size and tip offset for a handle, padding
/// the box for a comfortable finger touch target.
fn handle_shape(kind: HandleKind, radius: f32) -> HandleShape {
    let pad = HANDLE_TOUCH_PADDING.max(0.0);
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
    // Place the tip so the whole shape (plus padding) fits at non-negative
    // coordinates inside the box.
    let tip_in_box = Point {
        x: pad - bounds.x,
        y: pad - bounds.y,
    };
    let box_size = Size {
        width: bounds.width + 2.0 * pad,
        height: bounds.height + 2.0 * pad,
    };
    let path_data = handle_path_data(kind, tip_in_box.x, tip_in_box.y, radius);
    HandleShape {
        path_data,
        box_size,
        tip_in_box,
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
#[composable]
pub fn SelectionHandle(
    kind: HandleKind,
    tip: Point,
    radius: f32,
    color: Color,
    on_drag: impl Fn(Point) + 'static,
    on_drag_end: impl Fn() + 'static,
    on_long_press: impl Fn() + 'static,
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

    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let path_data = path_data.clone();
        let on_drag = Rc::clone(&on_drag);
        let on_drag_end = Rc::clone(&on_drag_end);
        let on_long_press = Rc::clone(&on_long_press);
        Box(
            Modifier::empty()
                .size(box_size)
                .draw_behind(move |scope: &mut dyn DrawScope| {
                    if let Ok(path) = VectorPath::parse(&path_data) {
                        scope.draw_vector_path(&path, Brush::solid(color));
                    }
                })
                .then(selection_handle_pointer_input(
                    Rc::clone(&on_drag),
                    Rc::clone(&on_drag_end),
                    Rc::clone(&on_long_press),
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
pub(crate) fn selection_handle_pointer_input(
    on_drag: Rc<dyn Fn(Point)>,
    on_drag_end: Rc<dyn Fn()>,
    on_long_press: Rc<dyn Fn()>,
) -> Modifier {
    Modifier::empty().pointer_input((), move |scope: PointerInputScope| {
        let on_drag = Rc::clone(&on_drag);
        let on_drag_end = Rc::clone(&on_drag_end);
        let on_long_press = Rc::clone(&on_long_press);
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
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                down_time = event.time_ms;
                                down_pos = event.global_position;
                                long_press_fired = false;
                                on_drag(event.global_position);
                                event.consume();
                            }
                            PointerEventKind::Move => {
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
                                on_drag_end();
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
