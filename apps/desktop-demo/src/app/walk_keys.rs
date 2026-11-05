// The keys that walk an eye through the demo's 3D tabs: W and S or the up
// and down arrows walk ahead and back, A and D step aside, and Q and E or
// the left and right arrows turn.

use cranpose_ui::{Key, KeyEvent, KeyEventType};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WalkKeys(u8);

impl WalkKeys {
    const FORWARD: u8 = 1;
    const BACK: u8 = 1 << 1;
    const LEFT: u8 = 1 << 2;
    const RIGHT: u8 = 1 << 3;
    const TURN_LEFT: u8 = 1 << 4;
    const TURN_RIGHT: u8 = 1 << 5;

    // The keys held after `event`, or `None` when its key walks nothing.
    pub(crate) fn after(self, event: &KeyEvent) -> Option<Self> {
        let bit = match event.key() {
            Key::W | Key::DirectionUp => Self::FORWARD,
            Key::S | Key::DirectionDown => Self::BACK,
            Key::A => Self::LEFT,
            Key::D => Self::RIGHT,
            Key::Q | Key::DirectionLeft => Self::TURN_LEFT,
            Key::E | Key::DirectionRight => Self::TURN_RIGHT,
            _ => return None,
        };
        Some(Self(if event.type_() == KeyEventType::KeyDown {
            self.0 | bit
        } else {
            self.0 & !bit
        }))
    }

    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    // 1 to walk ahead, -1 to walk back, 0 for both or neither.
    pub(crate) fn ahead(self) -> f32 {
        self.axis(Self::FORWARD, Self::BACK)
    }

    // 1 to step right, -1 to step left.
    pub(crate) fn right(self) -> f32 {
        self.axis(Self::RIGHT, Self::LEFT)
    }

    // 1 to turn right, -1 to turn left.
    pub(crate) fn turn(self) -> f32 {
        self.axis(Self::TURN_RIGHT, Self::TURN_LEFT)
    }

    fn axis(self, positive: u8, negative: u8) -> f32 {
        f32::from(u8::from(self.0 & positive != 0)) - f32::from(u8::from(self.0 & negative != 0))
    }
}
