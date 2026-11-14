// The capability flags of a modifier node: which specialized traits it
// implements.

use std::{
    fmt,
    ops::{BitOr, BitOrAssign},
};

use super::InvalidationKind;

/// Capability flags indicating which specialized traits a modifier node implements.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeCapabilities(u32);

impl NodeCapabilities {
    /// No capabilities.
    pub const NONE: Self = Self(0);
    /// Modifier participates in measure/layout.
    pub const LAYOUT: Self = Self(1 << 0);
    /// Modifier participates in draw.
    pub const DRAW: Self = Self(1 << 1);
    /// Modifier participates in pointer input.
    pub const POINTER_INPUT: Self = Self(1 << 2);
    /// Modifier participates in semantics tree construction.
    pub const SEMANTICS: Self = Self(1 << 3);
    /// Modifier participates in modifier locals.
    pub const MODIFIER_LOCALS: Self = Self(1 << 4);
    /// Modifier participates in focus management.
    pub const FOCUS: Self = Self(1 << 5);
    /// Modifier makes its node the root of a separate window: the node's
    /// subtree is laid out into that window's size and drawn into that
    /// window's scene, and the parent's scene skips it.
    pub const WINDOW_ROOT: Self = Self(1 << 6);
    /// Modifier handles hardware keyboard events.
    pub const KEY_INPUT: Self = Self(1 << 7);

    /// Returns an empty capability set.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Returns whether all bits in `other` are present in `self`.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns whether any bit in `other` is present in `self`.
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Inserts the requested capability bits.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Returns the raw bit representation.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Returns true when no capabilities are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns the capability bit mask required for the given invalidation.
    pub const fn for_invalidation(kind: InvalidationKind) -> Self {
        match kind {
            InvalidationKind::Layout => Self::LAYOUT,
            InvalidationKind::Draw => Self::DRAW,
            InvalidationKind::PointerInput => Self::POINTER_INPUT,
            InvalidationKind::Semantics => Self::SEMANTICS,
            InvalidationKind::Focus => Self::FOCUS,
        }
    }
}

impl Default for NodeCapabilities {
    fn default() -> Self {
        Self::NONE
    }
}

impl fmt::Debug for NodeCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeCapabilities")
            .field("layout", &self.contains(Self::LAYOUT))
            .field("draw", &self.contains(Self::DRAW))
            .field("pointer_input", &self.contains(Self::POINTER_INPUT))
            .field("semantics", &self.contains(Self::SEMANTICS))
            .field("modifier_locals", &self.contains(Self::MODIFIER_LOCALS))
            .field("focus", &self.contains(Self::FOCUS))
            .field("window_root", &self.contains(Self::WINDOW_ROOT))
            .field("key_input", &self.contains(Self::KEY_INPUT))
            .finish()
    }
}

impl BitOr for NodeCapabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for NodeCapabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
