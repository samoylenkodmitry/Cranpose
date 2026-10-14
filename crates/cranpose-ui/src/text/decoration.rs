use crate::modifier::Color;

/// A set of lines drawn on text: none, an underline, a line through, or both.
/// Combine values with [`TextDecoration::combine`]. Compose's `TextDecoration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextDecoration(u32);

impl TextDecoration {
    /// No line is drawn.
    pub const NONE: Self = Self(0);
    /// A line drawn below the text.
    pub const UNDERLINE: Self = Self(1);
    /// A line drawn through the middle of the text.
    pub const LINE_THROUGH: Self = Self(2);

    /// Returns true when every line set in `other` is also set in this value.
    pub fn contains(&self, other: Self) -> bool {
        (self.0 | other.0) == self.0
    }

    /// Returns a decoration that draws the lines of both `self` and `other`.
    pub fn combine(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for TextDecoration {
    fn default() -> Self {
        Self::NONE
    }
}

#[cfg(test)]
#[path = "tests/decoration_tests.rs"]
mod tests;

/// A shadow drawn behind text. Compose's `Shadow`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// Color of the shadow, including its alpha.
    pub color: Color,
    /// Offset of the shadow from the text, in logical pixels.
    pub offset: crate::modifier::Point,
    /// Blur radius of the shadow, in logical pixels. Zero gives a hard-edged shadow.
    pub blur_radius: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            color: Color(0.0, 0.0, 0.0, 1.0),
            offset: crate::modifier::Point::new(0.0, 0.0),
            blur_radius: 0.0,
        }
    }
}
