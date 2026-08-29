//! Pinch-to-zoom / pan state and the `Modifier::zoomable` gesture modifier.
//!
//! # Overview
//! [`ZoomState`] is a pure transform model (scale + offset) mirroring the
//! [`ScrollState`](crate::ScrollState) pattern: reactive `MutableState`
//! internals so composables and lazy `graphics_layer` closures observe
//! changes, plus non-reactive accessors for gesture handlers.
//!
//! The app renders the transform with the existing graphics-layer pipeline:
//!
//! ```text
//! let zoom = ZoomState::new();
//! Image(
//!     Modifier::empty()
//!         .fill_max_size()
//!         .zoomable(zoom.clone())
//!         .graphics_layer(move || zoom.layer()),
//!     ...
//! );
//! ```
//!
//! # Gestures
//! - **Two-finger pinch** (Android/touch): scale about the finger centroid,
//!   plus centroid pan. Activates immediately when the second finger lands.
//! - **One-finger pan**: activates after the drag threshold, and only while
//!   the content is zoomed in (`scale > 1`) so an unzoomed zoomable never
//!   steals drags from an enclosing scrollable.
//! - **Double tap**: resets a transformed state back to identity.
//! - **Ctrl+wheel / trackpad pinch** (desktop, web): discrete
//!   [`PointerEventKind::Zoom`] steps about the cursor.
//!
//! # Pan gating
//! ALL pan — one-finger drags and the two-finger pinch centroid — is inert
//! while `scale <= 1`: [`ZoomState::apply_transform`] clamps the offset back
//! to zero whenever the resulting scale is at (or below) identity, so a pinch
//! that zooms back out never strands the content at a stray offset.
//!
//! # Coordinate space
//! Gesture math runs in window (global) coordinates, which stay stable while
//! the element's own graphics layer is being transformed. The focal-point
//! anchoring is therefore exact for zoomable surfaces positioned at the
//! window origin (the fullscreen viewer case) and approximate otherwise.

use std::{
    cell::{Cell, RefCell},
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{MutableState, OwnedMutableState};
use cranpose_foundation::{
    DRAG_THRESHOLD,
    nodes::input::gestures::{TransformGesture, TransformGestureEvent},
};
use cranpose_ui_graphics::{GraphicsLayer, Point, TransformOrigin};
use web_time::Instant;

use crate::modifier::{Modifier, PointerEventKind};

const SCALE_EPSILON: f32 = 1e-3;

const DOUBLE_TAP_TIMEOUT_MS: i64 = 300;

const DOUBLE_TAP_SLOP: f32 = 100.0;

fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

/// Shared zoom/pan transform state for `Modifier::zoomable`.
///
/// Cloning shares the same underlying state (like `ScrollState`).
#[derive(Clone, Copy)]
pub struct ZoomState {
    inner: MutableState<Rc<ZoomStateInner>>,
}

struct ZoomStateInner {
    scale: OwnedMutableState<f32>,
    offset_x: OwnedMutableState<f32>,
    offset_y: OwnedMutableState<f32>,
    min_scale: Cell<f32>,
    max_scale: Cell<f32>,
}

impl Default for ZoomState {
    fn default() -> Self {
        Self::new()
    }
}

impl ZoomState {
    /// Creates a state at scale 1.0 with a 1.0..=8.0 zoom range.
    pub fn new() -> Self {
        Self::with_scale_range(1.0, 8.0)
    }

    /// Creates a state at scale 1.0 clamping zoom to `min_scale..=max_scale`.
    pub fn with_scale_range(min_scale: f32, max_scale: f32) -> Self {
        assert!(
            min_scale > 0.0 && max_scale >= min_scale,
            "invalid zoom range {min_scale}..{max_scale}"
        );
        let runtime = cranpose_core::current_runtime_handle()
            .expect("ZoomState::with_scale_range requires an active runtime");
        Self {
            inner: MutableState::with_runtime(
                Rc::new(ZoomStateInner {
                    scale: OwnedMutableState::with_runtime(
                        1.0f32.clamp(min_scale, max_scale),
                        runtime.clone(),
                    ),
                    offset_x: OwnedMutableState::with_runtime(0.0f32, runtime.clone()),
                    offset_y: OwnedMutableState::with_runtime(0.0f32, runtime.clone()),
                    min_scale: Cell::new(min_scale),
                    max_scale: Cell::new(max_scale),
                }),
                runtime,
            ),
        }
    }

    fn inner(&self) -> Rc<ZoomStateInner> {
        self.inner.get_non_reactive()
    }

    /// Stable identity of this state (shared across clones).
    pub fn id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.runtime_state_id().hash(&mut hasher);
        hasher.finish()
    }

    /// Current scale (reactive — subscribes the caller to changes).
    pub fn scale(&self) -> f32 {
        self.inner().scale.with(|s| *s)
    }

    /// Current scale without snapshot subscription.
    pub fn scale_non_reactive(&self) -> f32 {
        self.inner().scale.get_non_reactive()
    }

    /// Current pan offset in dp (reactive).
    pub fn offset(&self) -> Point {
        let inner = self.inner();
        Point {
            x: inner.offset_x.with(|v| *v),
            y: inner.offset_y.with(|v| *v),
        }
    }

    /// Current pan offset without snapshot subscription.
    pub fn offset_non_reactive(&self) -> Point {
        let inner = self.inner();
        Point {
            x: inner.offset_x.get_non_reactive(),
            y: inner.offset_y.get_non_reactive(),
        }
    }

    pub fn min_scale(&self) -> f32 {
        self.inner().min_scale.get()
    }

    pub fn max_scale(&self) -> f32 {
        self.inner().max_scale.get()
    }

    /// Whether the content is currently transformed away from identity.
    pub fn is_transformed(&self) -> bool {
        let offset = self.offset_non_reactive();
        (self.scale_non_reactive() - 1.0).abs() > SCALE_EPSILON
            || offset.x != 0.0
            || offset.y != 0.0
    }

    /// Whether the content is currently zoomed in beyond identity.
    ///
    /// Pan gestures only apply while this is `true`: an image at (or below)
    /// its natural size has nothing to pan, and drags over it belong to
    /// enclosing scrollables.
    pub fn is_zoomed_in(&self) -> bool {
        self.scale_non_reactive() > 1.0 + SCALE_EPSILON
    }

    /// Sets the scale directly (clamped to the configured range).
    pub fn set_scale(&self, scale: f32) {
        let clamped = scale.clamp(self.min_scale(), self.max_scale());
        self.inner().scale.set(clamped);
    }

    /// Sets the pan offset directly.
    pub fn set_offset(&self, offset: Point) {
        let inner = self.inner();
        inner.offset_x.set(offset.x);
        inner.offset_y.set(offset.y);
    }

    /// Resets to identity (scale clamped into range, zero offset).
    pub fn reset(&self) {
        self.set_scale(1.0);
        self.set_offset(Point { x: 0.0, y: 0.0 });
    }

    /// Applies one gesture step: zoom by `zoom` about `centroid`, then pan.
    ///
    /// `centroid` and `pan` are in the element's display frame; for finger
    /// gestures the anchor must be the centroid of the pointers BEFORE the
    /// step (as reported by `TransformGestureEvent::Transform`). With the
    /// top-left-origin layer produced by [`ZoomState::layer`], a content
    /// point `c` renders at `p = c * scale + offset`; this update applies
    /// `p' = zoom * (p - centroid) + centroid + pan`, which keeps the
    /// content glued to the fingers.
    ///
    /// Pan is inert while not zoomed in: whenever the resulting scale is at
    /// (or below) identity the offset is clamped back to zero, so a pinch
    /// that zooms out to `scale <= 1` — or a pure centroid pan at identity —
    /// never leaves the content stranded at a stray offset.
    pub fn apply_transform(&self, centroid: Point, pan: Point, zoom: f32) {
        let old_scale = self.scale_non_reactive();
        let new_scale = (old_scale * zoom).clamp(self.min_scale(), self.max_scale());
        let effective_zoom = new_scale / old_scale;
        let old_offset = self.offset_non_reactive();

        let new_offset = if new_scale <= 1.0 + SCALE_EPSILON {
            Point { x: 0.0, y: 0.0 }
        } else {
            Point {
                x: centroid.x - (centroid.x - old_offset.x) * effective_zoom + pan.x,
                y: centroid.y - (centroid.y - old_offset.y) * effective_zoom + pan.y,
            }
        };

        if new_scale != old_scale {
            self.inner().scale.set(new_scale);
        }
        if new_offset != old_offset {
            self.set_offset(new_offset);
        }
    }

    /// Builds the [`GraphicsLayer`] rendering this transform.
    ///
    /// Reads the state reactively, so it is meant for the lazy
    /// `Modifier::graphics_layer(move || state.layer())` form.
    pub fn layer(&self) -> GraphicsLayer {
        let scale = self.scale();
        let offset = self.offset();
        GraphicsLayer {
            scale_x: scale,
            scale_y: scale,
            translation_x: offset.x,
            translation_y: offset.y,
            transform_origin: TransformOrigin::new(0.0, 0.0),
            ..Default::default()
        }
    }
}

struct ZoomGestureState {
    tracker: TransformGesture,
    active: bool,
    travel: f32,
    tap_down: Option<Point>,
    last_tap: Option<(i64, Point)>,
    fallback_epoch: Instant,
}

impl Default for ZoomGestureState {
    fn default() -> Self {
        Self {
            tracker: TransformGesture::default(),
            active: false,
            travel: 0.0,
            tap_down: None,
            last_tap: None,
            fallback_epoch: Instant::now(),
        }
    }
}

impl ZoomGestureState {
    fn timestamp_ms(&self, time_ms: Option<i64>) -> i64 {
        time_ms.unwrap_or_else(|| self.fallback_epoch.elapsed().as_millis() as i64)
    }

    fn abandon_tap(&mut self) {
        self.tap_down = None;
        self.last_tap = None;
    }
}

impl Modifier {
    /// Makes the element respond to transform gestures, updating `state`.
    ///
    /// Recognizes two-finger pinch/pan (touch), one-finger pan while the
    /// content is zoomed in (`scale > 1`), a double-tap that resets a
    /// transformed state to identity, and [`PointerEventKind::Zoom`] steps
    /// (desktop ctrl+wheel, browser pinch). Pan — including the pinch
    /// centroid — is inert while `scale <= 1`. Rendering is the app's
    /// choice — typically `.graphics_layer(move || state.layer())` on the
    /// same or a child element.
    pub fn zoomable(self, state: ZoomState) -> Self {
        let gesture_state = Rc::new(RefCell::new(ZoomGestureState::default()));
        let key = state.id();

        self.pointer_input(key, move |scope| {
            let gesture_state = gesture_state.clone();

            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;

                            match event.kind {
                                PointerEventKind::Zoom => {
                                    if !event.is_consumed() && event.zoom_delta != 1.0 {
                                        state.apply_transform(
                                            event.global_position,
                                            Point { x: 0.0, y: 0.0 },
                                            event.zoom_delta,
                                        );
                                        event.consume();
                                    }
                                }
                                PointerEventKind::Cancel => {
                                    let mut gs = gesture_state.borrow_mut();
                                    gs.tracker.reset();
                                    gs.active = false;
                                    gs.travel = 0.0;
                                    gs.abandon_tap();
                                }
                                PointerEventKind::Down
                                | PointerEventKind::Move
                                | PointerEventKind::Up => {
                                    let mut gs = gesture_state.borrow_mut();

                                    if event.is_consumed() {
                                        gs.tracker.reset();
                                        gs.active = false;
                                        gs.travel = 0.0;
                                        gs.abandon_tap();
                                        continue;
                                    }

                                    let tracked =
                                        event.copy_with_local_position(event.global_position);
                                    let step = gs.tracker.handle_event(&tracked);

                                    if event.kind == PointerEventKind::Down {
                                        if gs.tracker.pointer_count() >= 2 {
                                            gs.active = true;
                                            gs.tap_down = None;
                                        } else {
                                            gs.travel = 0.0;
                                            gs.tap_down = Some(event.global_position);
                                        }
                                        if event.id != 0 {
                                            event.consume();
                                        }
                                        continue;
                                    }

                                    if event.kind == PointerEventKind::Up && event.id == 0 {
                                        let now_ms = gs.timestamp_ms(event.time_ms);
                                        let up_position = event.global_position;
                                        let is_tap = !gs.active
                                            && gs.tap_down.is_some_and(|down| {
                                                distance(down, up_position) <= DRAG_THRESHOLD
                                            });
                                        gs.tap_down = None;
                                        if is_tap {
                                            let is_double_tap = gs.last_tap.is_some_and(
                                                |(tap_ms, tap_position)| {
                                                    now_ms.saturating_sub(tap_ms)
                                                        <= DOUBLE_TAP_TIMEOUT_MS
                                                        && distance(tap_position, up_position)
                                                            <= DOUBLE_TAP_SLOP
                                                },
                                            );
                                            if is_double_tap {
                                                gs.last_tap = None;
                                                if state.is_transformed() {
                                                    state.reset();
                                                    event.consume();
                                                }
                                            } else {
                                                gs.last_tap = Some((now_ms, up_position));
                                            }
                                        } else {
                                            gs.last_tap = None;
                                        }
                                    }

                                    match step {
                                        TransformGestureEvent::Transform {
                                            pan,
                                            zoom,
                                            centroid,
                                            pointer_count,
                                        } => {
                                            if pointer_count >= 2 {
                                                gs.active = true;
                                            } else if !gs.active && state.is_zoomed_in() {
                                                gs.travel += (pan.x * pan.x + pan.y * pan.y).sqrt();
                                                if gs.travel > DRAG_THRESHOLD {
                                                    gs.active = true;
                                                }
                                            }

                                            if gs.active {
                                                state.apply_transform(centroid, pan, zoom);
                                                event.consume();
                                            }
                                        }
                                        TransformGestureEvent::Ended => {
                                            let was_active = gs.active;
                                            gs.active = false;
                                            gs.travel = 0.0;
                                            if was_active {
                                                event.consume();
                                            }
                                        }
                                        TransformGestureEvent::None => {
                                            if event.id != 0
                                                || (gs.active
                                                    && matches!(
                                                        event.kind,
                                                        PointerEventKind::Up
                                                            | PointerEventKind::Cancel
                                                    ))
                                            {
                                                event.consume();
                                            }
                                        }
                                    }
                                }
                                PointerEventKind::Scroll
                                | PointerEventKind::RotaryScrollPre
                                | PointerEventKind::RotaryScroll
                                | PointerEventKind::Enter
                                | PointerEventKind::Exit => {}
                            }
                        }
                    })
                    .await;
            }
        })
    }
}

#[cfg(test)]
#[path = "tests/zoom_tests.rs"]
mod tests;
