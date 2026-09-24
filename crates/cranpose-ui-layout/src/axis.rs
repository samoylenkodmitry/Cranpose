/// Represents the primary axis of flex layout (Row or Column).
///
/// This enum is used by FlexMeasurePolicy to determine which direction
/// is the main axis (where children are laid out) and which is the cross axis
/// (where children are aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Horizontal main axis (Row).
    /// Main axis: left to right
    /// Cross axis: top to bottom
    Horizontal,

    /// Vertical main axis (Column).
    /// Main axis: top to bottom
    /// Cross axis: left to right
    Vertical,
}

impl Axis {
    /// Returns the opposite axis.
    #[inline]
    pub fn cross_axis(self) -> Self {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    /// Returns true if this is the horizontal axis.
    #[inline]
    pub fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    /// Returns true if this is the vertical axis.
    #[inline]
    pub fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }
}

#[cfg(test)]
#[path = "tests/axis_tests.rs"]
mod tests;
