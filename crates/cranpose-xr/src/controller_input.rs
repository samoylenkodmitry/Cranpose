//! The inputs of a controller that can press a pointer button.

/// An input of a controller that a [`PointerMapping`](crate::PointerMapping) can map to a pointer
/// button.
///
/// The names follow [`Controller`](crate::Controller). Jetpack XR has no such type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControllerInput {
    /// The trigger under the index finger.
    Trigger,
    /// The grip button under the other fingers.
    Squeeze,
    /// The first face button: A on the right hand, X on the left.
    Primary,
    /// The second face button: B on the right hand, Y on the left.
    Secondary,
    /// The click of the thumbstick, or of the trackpad that stands in for it.
    ThumbstickClick,
    /// The menu button.
    Menu,
}

impl ControllerInput {
    /// Every input, in the order of the variants.
    pub const ALL: [Self; 6] = [
        Self::Trigger,
        Self::Squeeze,
        Self::Primary,
        Self::Secondary,
        Self::ThumbstickClick,
        Self::Menu,
    ];

    // The input's place in an array that holds one entry for each input.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Trigger => 0,
            Self::Squeeze => 1,
            Self::Primary => 2,
            Self::Secondary => 3,
            Self::ThumbstickClick => 4,
            Self::Menu => 5,
        }
    }
}
