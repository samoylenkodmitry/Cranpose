use cranpose_ui_graphics::Point;

use crate::{
    VelocityTracker1D,
    gesture_constants::MAX_FLING_VELOCITY,
    nodes::input::{PointerEvent, PointerEventKind, PointerId},
};

/// What a `FlingGesture` reports for one pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlingGestureEvent {
    /// The event ends no fling.
    None,
    /// The pointer lifted while moving. The value is the velocity on each axis, in logical pixels
    /// per second.
    Fling(Point),
    /// The system canceled the gesture.
    Canceled,
}

/// Tracks the velocity of one pointer and reports a fling when the pointer lifts while moving.
#[derive(Clone)]
pub struct FlingGesture {
    pointer_id: Option<PointerId>,
    x_tracker: VelocityTracker1D,
    y_tracker: VelocityTracker1D,
}

impl Default for FlingGesture {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FlingGesture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlingGesture")
            .field("pointer_id", &self.pointer_id)
            .finish_non_exhaustive()
    }
}

impl FlingGesture {
    /// Creates a gesture that tracks no pointer.
    pub fn new() -> Self {
        Self {
            pointer_id: None,
            x_tracker: VelocityTracker1D::new(),
            y_tracker: VelocityTracker1D::new(),
        }
    }

    /// Feeds one pointer event with its timestamp `time_ms`, in milliseconds, and returns a fling
    /// when the tracked pointer lifts while moving.
    pub fn handle_event(&mut self, event: &PointerEvent, time_ms: i64) -> FlingGestureEvent {
        match event.kind {
            PointerEventKind::Down if !event.is_consumed() => {
                self.pointer_id = Some(event.id);
                self.x_tracker.reset();
                self.y_tracker.reset();
                self.add_position(time_ms, event.position);
                FlingGestureEvent::None
            }
            PointerEventKind::Move if self.pointer_id == Some(event.id) => {
                self.add_position(time_ms, event.position);
                FlingGestureEvent::None
            }
            PointerEventKind::Up if self.pointer_id == Some(event.id) => {
                self.add_position(time_ms, event.position);
                let velocity = self.velocity();
                self.reset();
                if velocity.x == 0.0 && velocity.y == 0.0 {
                    FlingGestureEvent::None
                } else {
                    FlingGestureEvent::Fling(velocity)
                }
            }
            PointerEventKind::Cancel if self.pointer_id == Some(event.id) => {
                self.reset();
                FlingGestureEvent::Canceled
            }
            _ => FlingGestureEvent::None,
        }
    }

    /// The velocity of the tracked pointer on each axis, in logical pixels per second, capped at
    /// `MAX_FLING_VELOCITY`.
    pub fn velocity(&self) -> Point {
        Point {
            x: self
                .x_tracker
                .calculate_velocity_with_max(MAX_FLING_VELOCITY),
            y: self
                .y_tracker
                .calculate_velocity_with_max(MAX_FLING_VELOCITY),
        }
    }

    /// Forgets the tracked pointer and its recorded movement.
    pub fn reset(&mut self) {
        self.pointer_id = None;
        self.x_tracker.reset();
        self.y_tracker.reset();
    }

    fn add_position(&mut self, time_ms: i64, position: Point) {
        self.x_tracker.add_data_point(time_ms, position.x);
        self.y_tracker.add_data_point(time_ms, position.y);
    }
}

#[cfg(test)]
#[path = "tests/fling_tests.rs"]
mod tests;
