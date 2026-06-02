use crate::gesture_constants::DRAG_THRESHOLD;
use crate::nodes::input::{PointerEvent, PointerEventKind, PointerId};
use cranpose_ui_graphics::Point;

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
mod tests {
    use super::*;

    fn pointer(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
        PointerEvent::new(kind, Point { x, y }, Point { x, y })
    }

    #[test]
    fn drag_starts_after_threshold_and_consumes_move() {
        let mut gesture = DragGesture::new();
        let down = pointer(PointerEventKind::Down, 0.0, 0.0);
        let move_inside_slop = pointer(PointerEventKind::Move, DRAG_THRESHOLD - 1.0, 0.0);
        let move_after_slop = pointer(PointerEventKind::Move, DRAG_THRESHOLD + 1.0, 0.0);

        assert_eq!(gesture.handle_event(&down), DragGestureEvent::None);
        assert_eq!(
            gesture.handle_event(&move_inside_slop),
            DragGestureEvent::None
        );
        assert!(!move_inside_slop.is_consumed());
        assert_eq!(
            gesture.handle_event(&move_after_slop),
            DragGestureEvent::Started {
                start: Point { x: 0.0, y: 0.0 },
                current: Point {
                    x: DRAG_THRESHOLD + 1.0,
                    y: 0.0
                },
            }
        );
        assert!(move_after_slop.is_consumed());
    }

    #[test]
    fn drag_reports_delta_after_start() {
        let mut gesture = DragGesture::new();
        gesture.handle_event(&pointer(PointerEventKind::Down, 0.0, 0.0));
        gesture.handle_event(&pointer(PointerEventKind::Move, DRAG_THRESHOLD + 1.0, 0.0));

        assert_eq!(
            gesture.handle_event(&pointer(PointerEventKind::Move, DRAG_THRESHOLD + 3.0, 2.0)),
            DragGestureEvent::Dragged {
                delta: Point { x: 2.0, y: 2.0 },
                total: Point {
                    x: DRAG_THRESHOLD + 3.0,
                    y: 2.0
                },
                current: Point {
                    x: DRAG_THRESHOLD + 3.0,
                    y: 2.0
                },
            }
        );
    }
}
