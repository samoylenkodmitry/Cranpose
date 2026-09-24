//! Multi-pointer transform (pinch/pan) gesture recognition.
//!
//! [`TransformGesture`] tracks the positions of every active pointer and
//! reports incremental pan and zoom steps as individual pointer samples
//! arrive. Unlike Jetpack Compose — where each `PointerEvent` carries a full
//! frame of all pointers — cranpose delivers one event per pointer sample, so
//! the tracker keeps the last known position of every pointer and computes
//! each step against that stored snapshot:
//!
//! - **pan** is the movement of the centroid of all active pointers,
//! - **zoom** is the ratio of the mean pointer distance from the centroid
//!   after vs. before the sample (only meaningful with 2+ pointers).
//!
//! Individual steps of a two-finger pan wobble the zoom slightly (moving one
//! finger changes the spread), but consecutive steps compose to the exact
//! frame result: applying all `zoom` factors multiplicatively and all `pan`
//! deltas additively reproduces the true gesture.

use cranpose_ui_graphics::Point;

use crate::nodes::input::{PointerEvent, PointerEventKind, PointerId};

const MIN_ZOOM_SPREAD: f32 = 1.0;

/// A transform step reported by [`TransformGesture::handle_event`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformGestureEvent {
    /// Nothing to report for this event.
    None,
    /// The tracked pointer set moved.
    Transform {
        /// Centroid movement since the previous sample.
        pan: Point,
        /// Multiplicative spread change since the previous sample
        /// (`1.0` when fewer than two pointers are down).
        zoom: f32,
        /// Centroid of all active pointers BEFORE this sample, in the same
        /// coordinates as the pointer events. This is the anchor to zoom
        /// about: applying `display' = zoom * (display - centroid) +
        /// centroid + pan` keeps the content glued to the fingers (it
        /// matches Compose's `calculateCentroid(useCurrent = false)`).
        centroid: Point,
        /// Number of pointers taking part in the gesture.
        pointer_count: usize,
    },
    /// The last tracked pointer lifted; the gesture is over.
    Ended,
}

/// Tracks active pointers and recognizes pinch/pan transform steps.
#[derive(Clone, Debug, Default)]
pub struct TransformGesture {
    pointers: Vec<(PointerId, Point)>,
}

impl TransformGesture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of pointers currently tracked.
    pub fn pointer_count(&self) -> usize {
        self.pointers.len()
    }

    /// Centroid of the currently tracked pointers, if any.
    pub fn centroid(&self) -> Option<Point> {
        if self.pointers.is_empty() {
            None
        } else {
            Some(centroid_of(&self.pointers))
        }
    }

    /// Feeds one pointer event; returns the recognized transform step.
    pub fn handle_event(&mut self, event: &PointerEvent) -> TransformGestureEvent {
        match event.kind {
            PointerEventKind::Down => {
                match self.pointers.iter_mut().find(|(id, _)| *id == event.id) {
                    Some(entry) => entry.1 = event.position,
                    None => self.pointers.push((event.id, event.position)),
                }
                TransformGestureEvent::None
            }
            PointerEventKind::Move => {
                let Some(index) = self.pointers.iter().position(|(id, _)| *id == event.id) else {
                    return TransformGestureEvent::None;
                };

                let old_centroid = centroid_of(&self.pointers);
                let old_spread = mean_spread(&self.pointers, old_centroid);

                self.pointers[index].1 = event.position;

                let new_centroid = centroid_of(&self.pointers);
                let new_spread = mean_spread(&self.pointers, new_centroid);

                let pan = Point {
                    x: new_centroid.x - old_centroid.x,
                    y: new_centroid.y - old_centroid.y,
                };
                let zoom = if self.pointers.len() >= 2
                    && old_spread > MIN_ZOOM_SPREAD
                    && new_spread > MIN_ZOOM_SPREAD
                {
                    new_spread / old_spread
                } else {
                    1.0
                };

                if pan.x == 0.0 && pan.y == 0.0 && zoom == 1.0 {
                    TransformGestureEvent::None
                } else {
                    TransformGestureEvent::Transform {
                        pan,
                        zoom,
                        centroid: old_centroid,
                        pointer_count: self.pointers.len(),
                    }
                }
            }
            PointerEventKind::Up | PointerEventKind::Cancel => {
                self.pointers.retain(|(id, _)| *id != event.id);
                if self.pointers.is_empty() {
                    TransformGestureEvent::Ended
                } else {
                    TransformGestureEvent::None
                }
            }
            _ => TransformGestureEvent::None,
        }
    }

    /// Forgets all tracked pointers.
    pub fn reset(&mut self) {
        self.pointers.clear();
    }
}

fn centroid_of(pointers: &[(PointerId, Point)]) -> Point {
    let count = pointers.len() as f32;
    let mut sum = Point { x: 0.0, y: 0.0 };
    for (_, position) in pointers {
        sum.x += position.x;
        sum.y += position.y;
    }
    Point {
        x: sum.x / count,
        y: sum.y / count,
    }
}

fn mean_spread(pointers: &[(PointerId, Point)], centroid: Point) -> f32 {
    let count = pointers.len() as f32;
    let mut sum = 0.0;
    for (_, position) in pointers {
        let dx = position.x - centroid.x;
        let dy = position.y - centroid.y;
        sum += (dx * dx + dy * dy).sqrt();
    }
    sum / count
}

#[cfg(test)]
#[path = "tests/transform_tests.rs"]
mod tests;
