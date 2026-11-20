//! The two hands of a user.

/// A hand of the user: the one that holds a [`Controller`](crate::Controller) and takes a
/// vibration from [`Haptics`](crate::Haptics).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hand {
    /// The left hand.
    Left,
    /// The right hand.
    Right,
}

impl Hand {
    // The hands in the order of every array that holds one entry for each hand.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) const BOTH: [Self; 2] = [Self::Left, Self::Right];

    // The id of the pointer that the hand drives. Each hand has its own, and neither is the primary
    // pointer of a window with a mouse or a finger.
    pub(crate) const fn pointer_id(self) -> cranpose::PointerId {
        cranpose::PointerId(1 + self.index() as u64)
    }

    // The hand's place in an array that holds one entry for each hand.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}
