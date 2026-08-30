use crate::modifier::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextDecoration(u32);

impl TextDecoration {
    pub const NONE: Self = Self(0);
    pub const UNDERLINE: Self = Self(1);
    pub const LINE_THROUGH: Self = Self(2);

    pub fn contains(&self, other: Self) -> bool {
        (self.0 | other.0) == self.0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_requires_all_bits_from_other_decoration() {
        let combined = TextDecoration::UNDERLINE.combine(TextDecoration::LINE_THROUGH);

        assert!(combined.contains(TextDecoration::UNDERLINE));
        assert!(combined.contains(TextDecoration::LINE_THROUGH));
        assert!(combined.contains(combined));
        assert!(!TextDecoration::UNDERLINE.contains(combined));
        assert!(!TextDecoration::LINE_THROUGH.contains(combined));
    }

    #[test]
    fn none_contains_only_none() {
        assert!(TextDecoration::NONE.contains(TextDecoration::NONE));
        assert!(TextDecoration::UNDERLINE.contains(TextDecoration::NONE));
        assert!(!TextDecoration::NONE.contains(TextDecoration::UNDERLINE));
        assert!(!TextDecoration::NONE.contains(TextDecoration::LINE_THROUGH));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: Color,
    pub offset: crate::modifier::Point,
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
