// What an update of retained layers changed that outlives the frame it
// draws, as two bits: a walk folds each child's changes into its parent's
// with one `|`. The four values leave `Option<GraphChanges>` a byte, as
// `Option<bool>` is.

use std::ops::{BitOr, BitOrAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum GraphChanges {
    #[default]
    Unchanged = 0,
    // The hit targets must be collected again.
    Hits = 1,
    // A layer moved in its parent by its own properties, as an animated
    // graphics layer or offset moves it, which no layout pass reports.
    Moves = 2,
    HitsAndMoves = 3,
}

impl GraphChanges {
    const fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Unchanged,
            1 => Self::Hits,
            2 => Self::Moves,
            _ => Self::HitsAndMoves,
        }
    }

    pub(super) const fn hits(hits: bool) -> Self {
        Self::from_bits(hits as u8)
    }

    pub(super) const fn moves(moves: bool) -> Self {
        Self::from_bits((moves as u8) << 1)
    }

    pub(super) const fn contains(self, changes: Self) -> bool {
        self as u8 & changes as u8 == changes as u8
    }
}

impl BitOr for GraphChanges {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self::from_bits(self as u8 | other as u8)
    }
}

impl BitOrAssign for GraphChanges {
    fn bitor_assign(&mut self, other: Self) {
        *self = *self | other;
    }
}
