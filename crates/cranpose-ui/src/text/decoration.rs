use crate::modifier::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextDecoration(u32);

impl TextDecoration {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const LINE_THROUGH: Self = Self(2);

    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    pub fn combine(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for TextDecoration {
    fn default() -> Self {
        Self::NONE
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub offset: crate::modifier::Point, // Using Point as Offset counterpart
    pub blur_radius: f32,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            color: Color(0.0, 0.0, 0.0, 1.0), // Black default
            offset: crate::modifier::Point::new(0.0, 0.0),
            blur_radius: 0.0,
        }
    }
}
