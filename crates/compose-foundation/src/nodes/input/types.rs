use compose_ui_graphics::Point;
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
    pub buttons: PointerButtons,
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
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position,
            global_position,
            buttons: PointerButtons::NONE,
            consumed: Rc::new(Cell::new(false)),
        }
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
            buttons: self.buttons,
            consumed: self.consumed.clone(),
        }
    }
}
