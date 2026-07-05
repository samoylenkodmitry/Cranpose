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
use cranpose_foundation::PointerEventKind;
use cranpose_ui_graphics::{Brush, Color, DrawScope, Point, Rect, Size, VectorPath};

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
#[composable]
pub fn SelectionHandle(
    kind: HandleKind,
    tip: Point,
    radius: f32,
    color: Color,
    on_drag: impl Fn(Point) + 'static,
    on_drag_end: impl Fn() + 'static,
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

    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let path_data = path_data.clone();
        let on_drag = Rc::clone(&on_drag);
        let on_drag_end = Rc::clone(&on_drag_end);
        Box(
            Modifier::empty()
                .size(box_size)
                .draw_behind(move |scope: &mut dyn DrawScope| {
                    if let Ok(path) = VectorPath::parse(&path_data) {
                        scope.draw_vector_path(&path, Brush::solid(color));
                    }
                })
                .pointer_input((), move |scope: PointerInputScope| {
                    let on_drag = Rc::clone(&on_drag);
                    let on_drag_end = Rc::clone(&on_drag_end);
                    async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down | PointerEventKind::Move => {
                                            on_drag(event.global_position);
                                            event.consume();
                                        }
                                        PointerEventKind::Up | PointerEventKind::Cancel => {
                                            on_drag_end();
                                            event.consume();
                                        }
                                        _ => {}
                                    }
                                }
                            })
                            .await;
                    }
                }),
            BoxSpec::default(),
            || {},
        );
    });
}
