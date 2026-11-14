//! Which way a layout reads.

/// Which side of the interface is the start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayoutDirection {
    /// Start is the left edge — Latin, Cyrillic, CJK.
    #[default]
    Ltr,
    /// Start is the right edge — Arabic, Hebrew.
    Rtl,
}

impl LayoutDirection {
    /// Converts a distance from the start edge to a physical horizontal position.
    pub fn place_x(self, start: f32, width: f32, item_width: f32) -> f32 {
        if self.is_rtl() {
            width - start - item_width
        } else {
            start
        }
    }
    /// Whether the start edge is the right one.
    pub fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }

    /// The direction that reads the other way.
    pub fn reversed(self) -> Self {
        match self {
            Self::Ltr => Self::Rtl,
            Self::Rtl => Self::Ltr,
        }
    }

    /// Turns start/end values into physical left/right values.
    pub fn resolve(self, start: f32, end: f32) -> (f32, f32) {
        match self {
            Self::Ltr => (start, end),
            Self::Rtl => (end, start),
        }
    }
}
