//! The pointer input of a surface: where a controller's ray meets it, as Compose's pointer events.

use std::rc::Rc;

use cranpose::{Point, PointerButton, PointerButtons, PointerEvent, PointerEventKind};

// What the pointer does at one target, in the target's own dp from its top left corner. The
// target is the app's composition, which reads the atlas's dp, or one surface.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the pointer of the headset backend makes them")
)]
pub(crate) enum Gesture {
    Move { x: f32, y: f32 },
    Press,
    Release,
    // The pointer left the target.
    Leave,
}

// What the pointer did on a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceGesture {
    pub(crate) surface: u64,
    pub(crate) gesture: Gesture,
}

// The callback that the app gave a surface for its pointer events.
pub(crate) type PointerSink = Rc<dyn Fn(PointerEvent)>;

// What a surface knows of its pointer, to tell Compose's events from the pointer's gestures.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SurfaceInput {
    // Where the pointer is on the surface, or `None` while it is away
    at: Option<(f32, f32)>,
    pressed: bool,
}

impl SurfaceInput {
    // The event of Compose that `gesture` makes, with the position in the surface's dp. The
    // first move over the surface enters it, and a release with no press makes none.
    pub(crate) fn event(&mut self, gesture: Gesture) -> Option<PointerEvent> {
        let (kind, (x, y)) = match gesture {
            Gesture::Move { x, y } => {
                let kind = if self.at.is_some() {
                    PointerEventKind::Move
                } else {
                    PointerEventKind::Enter
                };
                self.at = Some((x, y));
                (kind, (x, y))
            }
            Gesture::Press => {
                self.pressed = true;
                (PointerEventKind::Down, self.at?)
            }
            Gesture::Release => {
                let released = std::mem::take(&mut self.pressed);
                (PointerEventKind::Up, self.at.filter(|_| released)?)
            }
            Gesture::Leave => (PointerEventKind::Exit, self.at.take()?),
        };
        let position = Point::new(x, y);
        let buttons = if self.pressed {
            PointerButtons::new().with(PointerButton::Primary)
        } else {
            PointerButtons::NONE
        };
        Some(PointerEvent::new(kind, position, position).with_buttons(buttons))
    }
}
