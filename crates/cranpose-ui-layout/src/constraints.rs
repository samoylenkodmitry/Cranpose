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

/// The values one bound of incoming constraints may take, both ends
/// included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundRange {
    pub low: f32,
    pub high: f32,
}

impl BoundRange {
    /// Every value.
    pub const ANY: Self = Self {
        low: f32::NEG_INFINITY,
        high: f32::INFINITY,
    };

    /// Only `value`.
    pub const fn exactly(value: f32) -> Self {
        Self {
            low: value,
            high: value,
        }
    }

    /// Every value up to `value`.
    pub const fn up_to(value: f32) -> Self {
        Self {
            low: f32::NEG_INFINITY,
            high: value,
        }
    }

    /// Every value from `value` up.
    pub const fn from(value: f32) -> Self {
        Self {
            low: value,
            high: f32::INFINITY,
        }
    }

    pub fn contains(self, value: f32) -> bool {
        self.low <= value && value <= self.high
    }

    /// The values both ranges hold; `None` when they share none.
    pub fn intersect(self, other: Self) -> Option<Self> {
        let range = Self {
            low: self.low.max(other.low),
            high: self.high.min(other.high),
        };
        (range.low <= range.high).then_some(range)
    }

    /// The range of a bound that reaches content `inset` less, floored at
    /// zero, as padding hands its content: the range content held, in the
    /// bound around it.
    pub fn outset(self, inset: f32) -> Self {
        Self {
            low: if self.low <= 0.0 {
                f32::NEG_INFINITY
            } else {
                self.low + inset
            },
            high: self.high + inset,
        }
    }
}

/// The incoming constraints of one axis a measurement holds for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisHold {
    pub min: BoundRange,
    pub max: BoundRange,
}

impl AxisHold {
    /// Every min and max that keeps `size` within them: what a layout
    /// whose size does not depend on the bounds holds for.
    pub const fn sized(size: f32) -> Self {
        Self {
            min: BoundRange::up_to(size),
            max: BoundRange::from(size),
        }
    }

    /// Constraints whose min is `size` and whose max allows it: a layout
    /// that takes its min, as an empty one does.
    pub const fn at_min(size: f32) -> Self {
        Self {
            min: BoundRange::exactly(size),
            max: BoundRange::from(size),
        }
    }

    pub fn contains(self, min: f32, max: f32) -> bool {
        self.min.contains(min) && self.max.contains(max)
    }

    /// The constraints both holds allow; `None` when they share none.
    pub fn intersect(self, other: Self) -> Option<Self> {
        Some(Self {
            min: self.min.intersect(other.min)?,
            max: self.max.intersect(other.max)?,
        })
    }

    /// This hold of content inside `inset` of padding, in the constraints
    /// around the padding.
    pub fn outset(self, inset: f32) -> Self {
        Self {
            min: self.min.outset(inset),
            max: self.max.outset(inset),
        }
    }
}

/// The incoming constraints a measurement stays the same under. A node
/// whose inputs did not change answers any constraints inside with the
/// measurement it has, instead of measuring again: an animated width
/// otherwise measured again every fixed-size box and every text it does not
/// wrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstraintsHold {
    pub width: AxisHold,
    pub height: AxisHold,
}

impl ConstraintsHold {
    /// Every constraints that keeps `width` by `height` within them.
    pub const fn sized(width: f32, height: f32) -> Self {
        Self {
            width: AxisHold::sized(width),
            height: AxisHold::sized(height),
        }
    }

    /// Constraints whose mins are `width` by `height`: what a layout that
    /// takes its min constraints holds for.
    pub const fn at_min(width: f32, height: f32) -> Self {
        Self {
            width: AxisHold::at_min(width),
            height: AxisHold::at_min(height),
        }
    }

    pub fn contains(&self, constraints: Constraints) -> bool {
        self.width
            .contains(constraints.min_width, constraints.max_width)
            && self
                .height
                .contains(constraints.min_height, constraints.max_height)
    }

    /// The constraints both holds allow; `None` when they share none.
    pub fn intersect(self, other: Self) -> Option<Self> {
        Some(Self {
            width: self.width.intersect(other.width)?,
            height: self.height.intersect(other.height)?,
        })
    }
}

/// What the content a layout modifier wraps measured: its size and the
/// constraints that measure holds for, `None` when only the constraints it
/// had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WrappedHold {
    pub size: cranpose_ui_graphics::Size,
    pub hold: Option<ConstraintsHold>,
}
