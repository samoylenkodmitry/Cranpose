//! How far a finger pinches the thumb.

/// How far one finger pinches the thumb.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pinch {
    /// From 0 with the finger away from the thumb to 1 with the two together.
    pub strength: f32,
    /// Whether the runtime counts the finger as pinching, which it decides from a threshold with
    /// some hysteresis.
    pub pinching: bool,
}
