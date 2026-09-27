//! Alignment utilities for positioning content

use crate::round_to_px;

/// Compose's `BiasAlignment`: the offset that puts `child` at `bias` of
/// `available` (-1 the start, 0 the middle, 1 the end), on a whole device
/// pixel of `density` as Kotlin's `roundToInt` rounds it, and before the
/// start when the child is the larger.
pub fn bias_offset(bias: f32, available: f32, child: f32, density: f32) -> f32 {
    round_to_px((available - child) / 2.0 * (1.0 + bias), density)
}

/// Alignment across both axes used for positioning content within a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    /// Horizontal alignment component.
    pub horizontal: HorizontalAlignment,
    /// Vertical alignment component.
    pub vertical: VerticalAlignment,
}

impl Alignment {
    /// Creates a new [`Alignment`] from explicit horizontal and vertical components.
    pub const fn new(horizontal: HorizontalAlignment, vertical: VerticalAlignment) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }

    /// Align children to the top-start corner.
    pub const TOP_START: Self = Self::new(HorizontalAlignment::Start, VerticalAlignment::Top);

    /// Align children to the center of the parent.
    pub const CENTER: Self = Self::new(
        HorizontalAlignment::CenterHorizontally,
        VerticalAlignment::CenterVertically,
    );

    /// Align children to the bottom-end corner.
    pub const BOTTOM_END: Self = Self::new(HorizontalAlignment::End, VerticalAlignment::Bottom);
}

/// Alignment along the horizontal axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalAlignment {
    /// Align children to the leading edge.
    Start,
    /// Align children to the horizontal center.
    CenterHorizontally,
    /// Align children to the trailing edge.
    End,
}

impl HorizontalAlignment {
    /// Compose's bias for this alignment: -1 the start, 0 the middle, 1 the end.
    pub fn bias(&self) -> f32 {
        match self {
            HorizontalAlignment::Start => -1.0,
            HorizontalAlignment::CenterHorizontally => 0.0,
            HorizontalAlignment::End => 1.0,
        }
    }

    /// The offset of a `child` wide in `available` width, on the device
    /// pixel grid of `density`: see [`bias_offset`].
    pub fn align(&self, available: f32, child: f32, density: f32) -> f32 {
        bias_offset(self.bias(), available, child, density)
    }
}

/// Alignment along the vertical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalAlignment {
    /// Align children to the top edge.
    Top,
    /// Align children to the vertical center.
    CenterVertically,
    /// Align children to the bottom edge.
    Bottom,
}

impl VerticalAlignment {
    /// Compose's bias for this alignment: -1 the top, 0 the middle, 1 the bottom.
    pub fn bias(&self) -> f32 {
        match self {
            VerticalAlignment::Top => -1.0,
            VerticalAlignment::CenterVertically => 0.0,
            VerticalAlignment::Bottom => 1.0,
        }
    }

    /// The offset of a `child` tall in `available` height, on the device
    /// pixel grid of `density`: see [`bias_offset`].
    pub fn align(&self, available: f32, child: f32, density: f32) -> f32 {
        bias_offset(self.bias(), available, child, density)
    }
}

#[cfg(test)]
#[path = "tests/alignment_tests.rs"]
mod tests;
