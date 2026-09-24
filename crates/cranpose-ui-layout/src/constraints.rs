//! Layout constraints system

/// Constraints used during layout measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    /// Creates constraints with exact width and height.
    pub fn tight(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    /// Creates constraints with loose bounds (min = 0, max = given values).
    pub fn loose(max_width: f32, max_height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width,
            min_height: 0.0,
            max_height,
        }
    }

    /// Returns true if these constraints have a single size that satisfies them.
    pub fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }

    /// Returns true if all bounds are finite.
    pub fn is_bounded(&self) -> bool {
        self.max_width.is_finite() && self.max_height.is_finite()
    }

    /// Constrains the provided width and height to fit within these constraints.
    pub fn constrain(&self, width: f32, height: f32) -> (f32, f32) {
        (
            width.clamp(self.min_width, self.max_width),
            height.clamp(self.min_height, self.max_height),
        )
    }

    /// Returns true if the width is bounded (max_width is finite).
    #[inline]
    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Returns true if the height is bounded (max_height is finite).
    #[inline]
    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Returns true if both width and height are tight (min == max for both).
    #[inline]
    pub fn has_tight_width(&self) -> bool {
        self.min_width == self.max_width
    }

    /// Returns true if the height is tight (min == max).
    #[inline]
    pub fn has_tight_height(&self) -> bool {
        self.min_height == self.max_height
    }

    /// Creates new constraints with tightened width (min = max = given width).
    pub fn tighten_width(self, width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            ..self
        }
    }

    /// Creates new constraints with tightened height (min = max = given height).
    pub fn tighten_height(self, height: f32) -> Self {
        Self {
            min_height: height,
            max_height: height,
            ..self
        }
    }

    /// Creates new constraints with the given width bounds.
    pub fn copy_with_width(self, min_width: f32, max_width: f32) -> Self {
        Self {
            min_width,
            max_width,
            ..self
        }
    }

    /// Creates new constraints with the given height bounds.
    pub fn copy_with_height(self, min_height: f32, max_height: f32) -> Self {
        Self {
            min_height,
            max_height,
            ..self
        }
    }

    /// Deflates constraints by the given amount on all sides.
    /// This is useful for applying padding before measuring children.
    pub fn deflate(self, horizontal: f32, vertical: f32) -> Self {
        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }

    /// Creates new constraints with loosened minimums (min = 0).
    pub fn loosen(self) -> Self {
        Self {
            min_width: 0.0,
            min_height: 0.0,
            ..self
        }
    }

    /// Creates constraints that enforce the given size.
    pub fn enforce(self, width: f32, height: f32) -> Self {
        Self {
            min_width: width.clamp(self.min_width, self.max_width),
            max_width: width.clamp(self.min_width, self.max_width),
            min_height: height.clamp(self.min_height, self.max_height),
            max_height: height.clamp(self.min_height, self.max_height),
        }
    }
}

#[cfg(test)]
#[path = "tests/constraints_tests.rs"]
mod tests;
