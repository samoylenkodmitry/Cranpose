use crate::gesture_constants::MAX_FLING_VELOCITY;
use crate::nodes::input::{PointerEvent, PointerEventKind, PointerId};
use crate::VelocityTracker1D;
use cranpose_ui_graphics::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlingGestureEvent {
    None,
    Fling(Point),
    Canceled,
}

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
    pub fn new() -> Self {
        Self {
            pointer_id: None,
            x_tracker: VelocityTracker1D::new(),
            y_tracker: VelocityTracker1D::new(),
        }
    }

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
mod tests {
    use super::*;

    fn pointer(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
        PointerEvent::new(kind, Point { x, y }, Point { x, y })
    }

    #[test]
    fn fling_reports_velocity_on_release() {
        let mut gesture = FlingGesture::new();
        gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0), 0);
        gesture.handle_event(&pointer(PointerEventKind::Move, 0.0, 40.0), 10);

        match gesture.handle_event(&pointer(PointerEventKind::Up, 0.0, 80.0), 20) {
            FlingGestureEvent::Fling(velocity) => {
                assert_eq!(velocity.x, 0.0);
                assert!(velocity.y > 0.0);
            }
            event => panic!("expected fling event, got {event:?}"),
        }
    }

    #[test]
    fn fling_resets_on_cancel() {
        let mut gesture = FlingGesture::new();
        gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0), 0);
        assert_eq!(
            gesture.handle_event(&pointer(PointerEventKind::Cancel, 0.0, 0.0), 1),
            FlingGestureEvent::Canceled
        );
        assert_eq!(gesture.velocity(), Point { x: 0.0, y: 0.0 });
    }
}
