use cranpose_ui_graphics::Point;
use std::cell::Cell;
use std::rc::Rc;

pub type PointerId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    Cancel,
    Scroll,
    /// Discrete zoom step (desktop ctrl+wheel, browser pinch-trackpad).
    /// The multiplicative factor is carried in [`PointerEvent::zoom_delta`].
    Zoom,
    Enter,
    Exit,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary = 0,
    Secondary = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerButtons(u8);

impl PointerButtons {
    pub const NONE: Self = Self(0);

    pub fn new() -> Self {
        Self::NONE
    }

    pub fn with(mut self, button: PointerButton) -> Self {
        self.insert(button);
        self
    }

    pub fn insert(&mut self, button: PointerButton) {
        self.0 |= 1 << (button as u8);
    }

    pub fn remove(&mut self, button: PointerButton) {
        self.0 &= !(1 << (button as u8));
    }

    pub fn contains(&self, button: PointerButton) -> bool {
        (self.0 & (1 << (button as u8))) != 0
    }
}

impl Default for PointerButtons {
    fn default() -> Self {
        Self::NONE
    }
}

/// Pointer event with consumption tracking for gesture disambiguation.
///
/// Events can be consumed by handlers (e.g., scroll) to prevent other handlers
/// (e.g., clicks) from receiving them. This enables proper gesture disambiguation
/// matching Jetpack Compose's event consumption pattern.
#[derive(Clone, Debug)]
pub struct PointerEvent {
    pub id: PointerId,
    pub kind: PointerEventKind,
    pub phase: PointerPhase,
    pub position: Point,
    pub global_position: Point,
    /// Scroll delta in logical pixels.
    ///
    /// This is non-zero for [`PointerEventKind::Scroll`] events and zero for
    /// button/move events.
    pub scroll_delta: Point,
    pub buttons: PointerButtons,
    /// Platform timestamp of the input sample in milliseconds, when available.
    ///
    /// The time base is platform specific (e.g. Android's uptime clock); only
    /// differences between events of the same gesture are meaningful. Gesture
    /// velocity trackers must prefer this over the delivery time because
    /// platforms like Android deliver input batched/frame-aligned: several
    /// samples arrive back-to-back and delivery-time stamping makes computed
    /// velocities wildly wrong.
    pub time_ms: Option<i64>,
    /// Multiplicative zoom factor for [`PointerEventKind::Zoom`] events
    /// (`> 1.0` zooms in, `< 1.0` zooms out). `1.0` for all other events.
    pub zoom_delta: f32,
    /// Tracks whether this event has been consumed by a handler.
    /// Shared via Rc<Cell> so consumption can be tracked across copies.
    consumed: Rc<Cell<bool>>,
}

impl PointerEvent {
    pub fn new(kind: PointerEventKind, position: Point, global_position: Point) -> Self {
        Self {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move | PointerEventKind::Enter | PointerEventKind::Exit => {
                    PointerPhase::Move
                }
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
                PointerEventKind::Scroll | PointerEventKind::Zoom => PointerPhase::Move,
            },
            position,
            global_position,
            scroll_delta: Point { x: 0.0, y: 0.0 },
            buttons: PointerButtons::NONE,
            time_ms: None,
            zoom_delta: 1.0,
            consumed: Rc::new(Cell::new(false)),
        }
    }

    /// Set the pointer id for this event (`0` is the primary pointer).
    pub fn with_id(mut self, id: PointerId) -> Self {
        self.id = id;
        self
    }

    /// Set the multiplicative zoom factor for a [`PointerEventKind::Zoom`] event.
    pub fn with_zoom_delta(mut self, zoom_delta: f32) -> Self {
        self.zoom_delta = zoom_delta;
        self
    }

    /// Set the scroll delta for this event.
    pub fn with_scroll_delta(mut self, scroll_delta: Point) -> Self {
        self.scroll_delta = scroll_delta;
        self
    }

    /// Set the platform timestamp (milliseconds) for this event.
    pub fn with_time_ms(mut self, time_ms: Option<i64>) -> Self {
        self.time_ms = time_ms;
        self
    }

    /// Set the buttons state for this event
    pub fn with_buttons(mut self, buttons: PointerButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Mark this event as consumed, preventing other handlers from processing it.
    ///
    /// Example: Scroll gestures consume events once dragging starts to prevent
    /// child buttons from firing clicks.
    pub fn consume(&self) {
        self.consumed.set(true);
    }

    /// Check if this event has been consumed by another handler.
    ///
    /// Handlers should check this before processing events. For example,
    /// clickable should not fire if the event was consumed by a scroll gesture.
    pub fn is_consumed(&self) -> bool {
        self.consumed.get()
    }

    /// Creates a copy of this event with a new local position, sharing the consumption state.
    pub fn copy_with_local_position(&self, position: Point) -> Self {
        Self {
            id: self.id,
            kind: self.kind,
            phase: self.phase,
            position,
            global_position: self.global_position,
            scroll_delta: self.scroll_delta,
            buttons: self.buttons,
            time_ms: self.time_ms,
            zoom_delta: self.zoom_delta,
            consumed: self.consumed.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn pointer_event_clones_share_consumed_state() {
        let event = PointerEvent::new(PointerEventKind::Move, point(1.0, 2.0), point(3.0, 4.0));
        let cloned = event.clone();
        assert!(!event.is_consumed());
        assert!(!cloned.is_consumed());

        cloned.consume();

        assert!(event.is_consumed());
        assert!(cloned.is_consumed());
    }

    #[test]
    fn pointer_event_copy_with_local_position_preserves_consumption_state() {
        let event = PointerEvent::new(PointerEventKind::Down, point(4.0, 5.0), point(4.0, 5.0));
        let local = event.copy_with_local_position(point(1.0, 1.0));

        assert_eq!(local.position, point(1.0, 1.0));
        assert_eq!(local.global_position, event.global_position);
        assert!(!local.is_consumed());

        event.consume();

        assert!(local.is_consumed());
    }
}
