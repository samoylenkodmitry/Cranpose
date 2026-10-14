use cranpose_ui_graphics::Point;

use crate::nodes::input::{PointerEvent, PointerEventKind};

/// What a `ScrollGesture` reports for one pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollGestureEvent {
    /// The event is not a scroll, was consumed already, or has a zero delta.
    None,
    /// A wheel or trackpad scroll. The value is the delta on each axis, in logical pixels.
    Scrolled(Point),
}

/// Recognizes wheel and trackpad scroll events. It holds no state.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct ScrollGesture;

impl ScrollGesture {
    /// Creates a scroll gesture.
    pub fn new() -> Self {
        Self
    }

    /// Returns the scroll delta of a scroll event that no handler consumed and that has a nonzero
    /// delta, and consumes the event.
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

    /// Does nothing. The gesture holds no state; the method gives it the same interface as the
    /// other gestures.
    pub fn reset(&mut self) {}
}

#[cfg(test)]
#[path = "tests/scroll_tests.rs"]
mod tests;
