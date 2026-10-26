/// A direction of layout or motion: the main axis of a row or a column, the
/// axis along which a scroller, a lazy list or a pager moves, and the axis a
/// nested scroll reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Left to right: a row's main axis, or content that moves left and right.
    Horizontal,

    /// Top to bottom: a column's main axis, or content that moves up and down.
    Vertical,
}

impl Axis {
    /// Returns the opposite axis.
    #[inline]
    pub fn cross_axis(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }

    /// Returns true if this is the horizontal axis.
    #[inline]
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Horizontal)
    }

    /// Returns true if this is the vertical axis.
    #[inline]
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Vertical)
    }
}

#[cfg(test)]
#[path = "tests/axis_tests.rs"]
mod tests;
