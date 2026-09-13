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
mod tests {
    use super::*;

    #[test]
    fn tint_accepts_calibration_levels_and_intermediate_values() {
        for percent in [0.0, 12.5, 20.0, 25.0, 50.0, 75.0, 100.0] {
            assert_eq!(
                GlassTintAmount::from_percent(percent).unwrap().percent(),
                percent
            );
        }
        assert_eq!(GlassTintAmount::default().percent(), 25.0);
    }

    #[test]
    fn tint_rejects_invalid_percentages() {
        for percent in [-0.01, 100.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                GlassTintAmount::from_percent(percent),
                Err(InvalidGlassTintAmount)
            );
        }
        assert_eq!(
            InvalidGlassTintAmount.to_string(),
            "glass tint amount must be finite and between 0 and 100 percent"
        );
    }
}
