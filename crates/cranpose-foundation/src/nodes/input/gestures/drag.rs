use cranpose_ui_graphics::Point;

use crate::{
    gesture_constants::DRAG_THRESHOLD,
    nodes::input::{PointerEvent, PointerEventKind, PointerId},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragGestureEvent {
    None,
    Started {
        start: Point,
        current: Point,
    },
    Dragged {
        delta: Point,
        total: Point,
        current: Point,
    },
    Ended,
    Canceled,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct DragGesture {
    pointer_id: Option<PointerId>,
    start_position: Point,
    last_position: Point,
    dragging: bool,
}

impl DragGesture {
    pub fn new() -> Self {
        Self::default()
    }

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
