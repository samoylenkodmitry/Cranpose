use cranpose_ui_graphics::Point;

use crate::nodes::input::{PointerEvent, PointerEventKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollGestureEvent {
    None,
    Scrolled(Point),
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct ScrollGesture;

impl ScrollGesture {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_event(&mut self, event: &PointerEvent) -> ScrollGestureEvent {
        if event.kind != PointerEventKind::Scroll || event.is_consumed() {
            return ScrollGestureEvent::None;
        }
        if event.scroll_delta.x == 0.0 && event.scroll_delta.y == 0.0 {
            return ScrollGestureEvent::None;
        }
        event.consume();
        ScrollGestureEvent::Scrolled(event.scroll_delta)
    }

    pub fn reset(&mut self) {}
}

#[cfg(test)]
#[path = "tests/scroll_tests.rs"]
mod tests;
