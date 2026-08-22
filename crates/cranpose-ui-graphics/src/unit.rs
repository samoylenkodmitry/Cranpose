//! Unit types: Dp, Sp, and conversions

/// Density-independent pixels
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Dp(pub f32);

impl Dp {
    pub fn to_px(&self, density: f32) -> f32 {
        self.0 * density
    }

    pub fn from_px(px: f32, density: f32) -> Self {
        Self(px / density)
    }
}

/// Scale-independent pixels (for text)
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Sp(pub f32);

impl Sp {
    pub fn to_px(&self, density: f32, font_scale: f32) -> f32 {
        self.0 * density * font_scale
    }

    pub fn from_px(px: f32, density: f32, font_scale: f32) -> Self {
        Self(px / (density * font_scale))
    }
}

/// Raw pixels
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Px(pub f32);

#[cfg(test)]
mod tests {
    use super::*;

    /// The two directions have to be each other's inverse, or a value that made
    /// a round trip through the platform — a touch slop measured in pixels,
    /// stored as `Dp`, applied back in pixels — comes back a different size on
    /// every screen but the one it was written on.
    #[test]
    fn density_independent_pixels_round_trip_through_a_density() {
        for density in [1.0f32, 1.5, 2.0, 3.0] {
            let dp = Dp(24.0);
            assert_eq!(dp.to_px(density), 24.0 * density);
            assert_eq!(Dp::from_px(dp.to_px(density), density), dp);
        }
    }

    /// Text carries the user's font-scale setting as well as the screen's
    /// density, and both have to survive the trip.
    #[test]
    fn scale_independent_pixels_round_trip_through_density_and_font_scale() {
        for density in [1.0f32, 2.0, 3.0] {
            for font_scale in [0.85f32, 1.0, 1.3] {
                let sp = Sp(16.0);
                assert_eq!(sp.to_px(density, font_scale), 16.0 * density * font_scale);
                assert_eq!(
                    Sp::from_px(sp.to_px(density, font_scale), density, font_scale),
                    sp
                );
            }
        }
    }
}
