//! Compose's `PaddingValues`: four edge insets in reading order.

use cranpose_ui_graphics::EdgeInsets;

use crate::{
    Modifier,
    layout_direction::{LayoutDirection, layout_direction},
};

/// Padding on four edges, in reading order, such as a scaffold's insets for
/// its content slot or a lazy list's content padding.
///
/// `start` and `end` are the reading-order edges: in a left-to-right layout
/// `start` is the left edge, in a right-to-left layout it is the right one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaddingValues {
    /// Padding on the reading-order start edge, in logical pixels.
    pub start: f32,
    /// Padding on the top edge, in logical pixels.
    pub top: f32,
    /// Padding on the reading-order end edge, in logical pixels.
    pub end: f32,
    /// Padding on the bottom edge, in logical pixels.
    pub bottom: f32,
}

impl PaddingValues {
    /// Creates padding values from the four edges, in logical pixels and reading order.
    pub const fn new(start: f32, top: f32, end: f32, bottom: f32) -> Self {
        Self {
            start,
            top,
            end,
            bottom,
        }
    }

    /// The same padding on every edge.
    pub const fn all(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    /// `horizontal` on the start and end edges, `vertical` on the top and
    /// bottom ones: Compose's `PaddingValues(horizontal, vertical)`.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(horizontal, vertical, horizontal, vertical)
    }

    /// Applies these values in the composition's current reading order.
    pub fn apply_to(self, modifier: Modifier) -> Modifier {
        self.apply_to_in(modifier, layout_direction())
    }

    /// Applies these values against an explicit reading order.
    pub fn apply_to_in(self, modifier: Modifier, direction: LayoutDirection) -> Modifier {
        modifier.padding_relative_in(direction, self.start, self.top, self.end, self.bottom)
    }

    /// The physical left/top/right/bottom edges in `direction`.
    pub fn physical(self, direction: LayoutDirection) -> EdgeInsets {
        let (left, right) = direction.resolve(self.start, self.end);
        EdgeInsets::from_components(left, self.top, right, self.bottom)
    }

    /// The larger of each edge, used to merge bar heights with window insets.
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.start.max(other.start),
            self.top.max(other.top),
            self.end.max(other.end),
            self.bottom.max(other.bottom),
        )
    }
}
