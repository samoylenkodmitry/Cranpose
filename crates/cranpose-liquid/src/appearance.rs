//! Application-wide Liquid Glass appearance preferences.

/// A tint percentage from clear (0) to fully tinted (100).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassTintAmount(f32);

/// A tint percentage was non-finite or outside the supported range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("glass tint amount must be finite and between 0 and 100 percent")]
pub struct InvalidGlassTintAmount;

impl GlassTintAmount {
    /// Validates a percentage from 0 through 100, inclusive.
    pub fn from_percent(percent: f32) -> Result<Self, InvalidGlassTintAmount> {
        if percent.is_finite() && (0.0..=100.0).contains(&percent) {
            Ok(Self(percent))
        } else {
            Err(InvalidGlassTintAmount)
        }
    }

    /// Returns the configured percentage.
    pub fn percent(self) -> f32 {
        self.0
    }
}

impl Default for GlassTintAmount {
    fn default() -> Self {
        Self(25.0)
    }
}

#[cfg(test)]
#[path = "tests/appearance_tests.rs"]
mod tests;
