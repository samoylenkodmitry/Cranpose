//! When a trigger, a grip or a pinch counts as pressed.

// A pull counts as down above PRESS and as up below RELEASE, so a value held near one does not
// flicker.
pub(crate) const PRESS: f32 = 0.6;
pub(crate) const RELEASE: f32 = 0.4;

// Whether a control was pressed in the frame before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Was {
    Up,
    Down,
}

impl From<bool> for Was {
    fn from(down: bool) -> Self {
        if down { Self::Down } else { Self::Up }
    }
}

// Whether a control is pressed at `value`, given how it was in the frame before.
pub(crate) fn pressed(value: f32, was: Was) -> bool {
    match was {
        Was::Down => value > RELEASE,
        Was::Up => value >= PRESS,
    }
}
