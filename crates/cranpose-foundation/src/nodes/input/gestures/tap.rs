use cranpose_ui_graphics::Point;

use crate::{
    gesture_constants::DRAG_THRESHOLD,
    nodes::input::{PointerEvent, PointerEventKind, PointerId},
};

/// What a `TapGesture` reports for one pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TapGestureEvent {
    /// The event does not change the tap.
    None,
    /// The pointer went down at this position.
    Pressed(Point),
    /// The pointer lifted without a drag and without another handler consuming the event, at this
    /// position.
    Tapped(Point),
    /// The tap will not happen: the pointer moved past the drag threshold, another handler consumed
    /// the event, or the system canceled it.
    Canceled,
}

/// Recognizes a tap by one pointer. A press that moves further than `DRAG_THRESHOLD` is no tap.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct TapGesture {
    pointer_id: Option<PointerId>,
    down_position: Point,
    canceled_by_drag: bool,
}

impl TapGesture {
    /// Creates a gesture that tracks no pointer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one pointer event and returns the tap step it causes.
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

    /// Forgets the tracked pointer.
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
