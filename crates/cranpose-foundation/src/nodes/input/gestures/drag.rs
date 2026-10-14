use cranpose_ui_graphics::Point;

use crate::{
    gesture_constants::DRAG_THRESHOLD,
    nodes::input::{PointerEvent, PointerEventKind, PointerId},
};

/// What a `DragGesture` reports for one pointer event. Positions are in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragGestureEvent {
    /// The event does not change the drag.
    None,
    /// The pointer moved further than the drag threshold from where it went down.
    Started {
        /// Where the pointer went down.
        start: Point,
        /// Where the pointer is now.
        current: Point,
    },
    /// The pointer moved while the drag is in progress.
    Dragged {
        /// Movement since the previous drag event.
        delta: Point,
        /// Movement since the pointer went down.
        total: Point,
        /// Where the pointer is now.
        current: Point,
    },
    /// The pointer lifted after a drag.
    Ended,
    /// The system canceled the gesture.
    Canceled,
}

/// Recognizes a drag by one pointer. A drag starts once the pointer is further than
/// `DRAG_THRESHOLD` from where it went down.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct DragGesture {
    pointer_id: Option<PointerId>,
    start_position: Point,
    last_position: Point,
    dragging: bool,
}

impl DragGesture {
    /// Creates a gesture that tracks no pointer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one pointer event and returns the drag step it causes. The events that start and
    /// continue a drag are consumed, so handlers such as clicks do not see them.
    pub fn handle_event(&mut self, event: &PointerEvent) -> DragGestureEvent {
        match event.kind {
            PointerEventKind::Down if !event.is_consumed() => {
                self.pointer_id = Some(event.id);
                self.start_position = event.position;
                self.last_position = event.position;
                self.dragging = false;
                DragGestureEvent::None
            }
            PointerEventKind::Move if self.pointer_id == Some(event.id) => {
                let total = delta(self.start_position, event.position);
                if !self.dragging {
                    if length(total) <= DRAG_THRESHOLD {
                        return DragGestureEvent::None;
                    }
                    self.dragging = true;
                    self.last_position = event.position;
                    event.consume();
                    return DragGestureEvent::Started {
                        start: self.start_position,
                        current: event.position,
                    };
                }

                let event_delta = delta(self.last_position, event.position);
                self.last_position = event.position;
                event.consume();
                DragGestureEvent::Dragged {
                    delta: event_delta,
                    total,
                    current: event.position,
                }
            }
            PointerEventKind::Up if self.pointer_id == Some(event.id) => {
                let was_dragging = self.dragging;
                self.reset();
                if was_dragging {
                    DragGestureEvent::Ended
                } else {
                    DragGestureEvent::None
                }
            }
            PointerEventKind::Cancel if self.pointer_id == Some(event.id) => {
                self.reset();
                DragGestureEvent::Canceled
            }
            _ => DragGestureEvent::None,
        }
    }

    /// Forgets the tracked pointer and any drag in progress.
    pub fn reset(&mut self) {
        self.pointer_id = None;
        self.start_position = Point { x: 0.0, y: 0.0 };
        self.last_position = Point { x: 0.0, y: 0.0 };
        self.dragging = false;
    }
}

fn delta(from: Point, to: Point) -> Point {
    Point {
        x: to.x - from.x,
        y: to.y - from.y,
    }
}

fn length(delta: Point) -> f32 {
    (delta.x * delta.x + delta.y * delta.y).sqrt()
}

#[cfg(test)]
#[path = "tests/drag_tests.rs"]
mod tests;
