use cranpose_ui_graphics::Point;

use crate::{
    gesture_constants::DRAG_THRESHOLD,
    nodes::input::{PointerEvent, PointerEventKind, PointerId},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TapGestureEvent {
    None,
    Pressed(Point),
    Tapped(Point),
    Canceled,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct TapGesture {
    pointer_id: Option<PointerId>,
    down_position: Point,
    canceled_by_drag: bool,
}

impl TapGesture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_event(&mut self, event: &PointerEvent) -> TapGestureEvent {
        match event.kind {
            PointerEventKind::Down if !event.is_consumed() => {
                self.pointer_id = Some(event.id);
                self.down_position = event.travelled_to();
                self.canceled_by_drag = false;
                TapGestureEvent::Pressed(event.position)
            }
            PointerEventKind::Move if self.pointer_id == Some(event.id) => {
                if !self.canceled_by_drag
                    && distance(self.down_position, event.travelled_to()) > DRAG_THRESHOLD
                {
                    self.canceled_by_drag = true;
                    TapGestureEvent::Canceled
                } else {
                    TapGestureEvent::None
                }
            }
            PointerEventKind::Up if self.pointer_id == Some(event.id) => {
                let tapped = !self.canceled_by_drag && !event.is_consumed();
                let position = event.position;
                self.reset();
                if tapped {
                    TapGestureEvent::Tapped(position)
                } else {
                    TapGestureEvent::Canceled
                }
            }
            PointerEventKind::Cancel if self.pointer_id == Some(event.id) => {
                self.reset();
                TapGestureEvent::Canceled
            }
            _ => TapGestureEvent::None,
        }
    }

    pub fn reset(&mut self) {
        self.pointer_id = None;
        self.down_position = Point { x: 0.0, y: 0.0 };
        self.canceled_by_drag = false;
    }
}

fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
#[path = "tests/tap_tests.rs"]
mod tests;
