//! Length units: [`Dp`], [`Sp`], [`Px`], and the [`Density`] that converts
//! between them.
//!
//! This mirrors Jetpack Compose's `Dp`/`TextUnit`/pixel split: a layout
//! author reasons in [`Dp`] (density-independent) and [`Sp`] (scale-
//! independent, for text), never in a raw device pixel count, and the only
//! way from either into [`Px`] is through an explicit [`Density`]. Neither
//! [`Dp`] nor [`Sp`] implements `From<Px>` (nor the reverse for [`Dp`]/
//! [`Sp`] into [`Px`] without a density), so a call site cannot hand a
//! pixel count to a `Dp`-typed parameter and have it silently compile:
//!
//! ```compile_fail
//! use cranpose_ui_graphics::{Dp, Px};
//!
//! fn takes_dp(_padding: impl Into<Dp>) {}
//!
//! let measured = Px(16.0);
//! takes_dp(measured); // Px has no `Into<Dp>` impl -- this does not compile.
//! ```
//!
//! Going the other way requires stating the grid:
//!
//! ```
//! use cranpose_ui_graphics::{Density, Dp};
//!
//! let padding = Dp(16.0);
//! let px = padding.to_px(Density::from_scale(2.0));
//! assert_eq!(px.0, 32.0);
//! ```

use std::ops::{Add, Div, Mul, Neg, Sub};

/// The device pixel grid a [`Dp`] or [`Sp`] value is measured against: how
/// many device pixels one [`Dp`] is worth, and how far the platform's text-
/// size setting scales an [`Sp`] beyond that.
///
/// This is the pure-data half of Compose's `Density`. The composition-aware
/// wrapper that reads the ambient grid from a running app and reacts to
/// [`CompositionLocalProvider`](https://docs.rs/cranpose-core)-style
/// overrides lives above this crate (`cranpose-ui`'s `Density`, which cannot
/// live here without an upward dependency on `cranpose-ui`) and converts
/// into this type at the boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Density {
    /// Device pixels per [`Dp`].
    pub scale: f32,
    /// The user's text-size setting, applied on top of `scale` for [`Sp`].
    pub font_scale: f32,
}

impl Density {
    /// One layout point per device pixel, no extra text scaling -- the grid
    /// a test or a golden reaches for when the host's real grid does not
    /// matter to what it is checking.
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        font_scale: 1.0,
    };

    pub fn new(scale: f32, font_scale: f32) -> Self {
        Self { scale, font_scale }
    }

    /// A grid whose text does not follow a separate font-scale setting --
    /// what a caller converting a length rather than a text size reaches
    /// for, since a length must not move when the user's text size does.
    pub fn from_scale(scale: f32) -> Self {
        Self::new(scale, 1.0)
    }
}

impl Default for Density {
    fn default() -> Self {
        Self::IDENTITY
    }
}

macro_rules! scalar_unit {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
        pub struct $name(pub f32);

        impl $name {
            pub const ZERO: Self = Self(0.0);

            /// The larger of two lengths in the same unit. Comparing across
            /// units (a `Dp` against an `Sp`) is exactly the mistake this
            /// module exists to make impossible, so this does not accept one.
            pub fn max(self, other: Self) -> Self {
                Self(self.0.max(other.0))
            }

            /// The smaller of two lengths in the same unit.
            pub fn min(self, other: Self) -> Self {
                Self(self.0.min(other.0))
            }
        }

        impl From<f32> for $name {
            fn from(value: f32) -> Self {
                Self(value)
            }
        }

        impl From<i32> for $name {
            fn from(value: i32) -> Self {
                Self(value as f32)
            }
        }

        impl Add for $name {
            type Output = Self;
            fn add(self, rhs: Self) -> Self {
                Self(self.0 + rhs.0)
            }
        }

        impl Sub for $name {
            type Output = Self;
            fn sub(self, rhs: Self) -> Self {
                Self(self.0 - rhs.0)
            }
        }

        impl Neg for $name {
            type Output = Self;
            fn neg(self) -> Self {
                Self(-self.0)
            }
        }

        impl Mul<f32> for $name {
            type Output = Self;
            fn mul(self, rhs: f32) -> Self {
                Self(self.0 * rhs)
            }
        }

        impl Div<f32> for $name {
            type Output = Self;
            fn div(self, rhs: f32) -> Self {
                Self(self.0 / rhs)
            }
        }
    };
}

scalar_unit!(
    Dp,
    "Density-independent pixels: a layout length that reads the same on \
     every device regardless of its pixel density."
);
scalar_unit!(
    Sp,
    "Scale-independent pixels: a text size that follows both the device's \
     pixel density and the user's font-scale accessibility setting. \
     Distinct from `Dp` because a length must not move when the user's \
     text-size setting does, and a text size must."
);
scalar_unit!(
    Px,
    "A device pixel. Renderer-facing surfaces, hit-testing, and \
     framebuffer coordinates read and write this; a layout author reaches \
     for `Dp`/`Sp` instead, and can only get here through an explicit \
     `Density`."
);

impl Dp {
    /// Converts to the actual device pixel count on `density`'s grid.
    pub fn to_px(self, density: Density) -> Px {
        Px(self.0 * density.scale)
    }

    /// Recovers the `Dp` that produced `px` on `density`'s grid.
    pub fn from_px(px: Px, density: Density) -> Self {
        Self(px.0 / density.scale)
    }
}

impl Sp {
    /// Converts to the actual device pixel count on `density`'s grid,
    /// scaled by the user's font-scale setting on top of pixel density.
    pub fn to_px(self, density: Density) -> Px {
        Px(self.0 * density.scale * density.font_scale)
    }

    /// Recovers the `Sp` that produced `px` on `density`'s grid.
    pub fn from_px(px: Px, density: Density) -> Self {
        Self(px.0 / (density.scale * density.font_scale))
    }
}

impl Px {
    /// Recovers the layout length that produced this pixel count on
    /// `density`'s grid.
    pub fn to_dp(self, density: Density) -> Dp {
        Dp(self.0 / density.scale)
    }
}

/// Reads `16.0.dp()` at a call site rather than `Dp(16.0)`, matching
/// Kotlin's `Float.dp`/`Int.dp` extension properties.
pub trait DpExt {
    fn dp(self) -> Dp;
}

impl DpExt for f32 {
    fn dp(self) -> Dp {
        Dp(self)
    }
}

impl DpExt for i32 {
    fn dp(self) -> Dp {
        Dp(self as f32)
    }
}

/// Reads `16.0.sp()` at a call site rather than `Sp(16.0)`.
pub trait SpExt {
    fn sp(self) -> Sp;
}

impl SpExt for f32 {
    fn sp(self) -> Sp {
        Sp(self)
    }
}

impl SpExt for i32 {
    fn sp(self) -> Sp {
        Sp(self as f32)
    }
}

/// Reads `16.0.px()` at a call site rather than `Px(16.0)`.
pub trait PxExt {
    fn px(self) -> Px;
}

impl PxExt for f32 {
    fn px(self) -> Px {
        Px(self)
    }
}

impl PxExt for i32 {
    fn px(self) -> Px {
        Px(self as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two directions have to be each other's inverse, or a value that
    /// made a round trip through the platform -- a touch slop measured in
    /// pixels, stored as `Dp`, applied back in pixels -- comes back a
    /// different size on every screen but the one it was written on.
    #[test]
    fn density_independent_pixels_round_trip_through_a_density() {
        for scale in [1.0f32, 1.5, 2.0, 3.0] {
            let density = Density::from_scale(scale);
            let dp = Dp(24.0);
            assert_eq!(dp.to_px(density), Px(24.0 * scale));
            assert_eq!(Dp::from_px(dp.to_px(density), density), dp);
        }
    }

    /// Pinned at a non-1.0 density so a regression that drops the `scale`
    /// factor (or silently treats `Dp` as already being in pixels) fails
    /// this test rather than only showing up as a soft, mis-sized edge on a
    /// real high-density screen.
    #[test]
    fn a_dp_length_is_pinned_at_a_non_identity_density() {
        let density = Density::from_scale(2.5);
        assert_eq!(Dp(16.0).to_px(density), Px(40.0));
        assert_eq!(Px(40.0).to_dp(density), Dp(16.0));
    }

    /// Text carries the user's font-scale setting as well as the screen's
    /// density, and both have to survive the trip.
    #[test]
    fn scale_independent_pixels_round_trip_through_density_and_font_scale() {
        for scale in [1.0f32, 2.0, 3.0] {
            for font_scale in [0.85f32, 1.0, 1.3] {
                let density = Density::new(scale, font_scale);
                let sp = Sp(16.0);
                assert_eq!(sp.to_px(density), Px(16.0 * scale * font_scale));
                assert_eq!(Sp::from_px(sp.to_px(density), density), sp);
            }
        }
    }

    /// Pinned at a non-1.0 density and a non-1.0 font scale together, since
    /// either one alone can hide a regression that mixes up which factor
    /// applies to which unit.
    #[test]
    fn an_sp_length_is_pinned_at_a_non_identity_density_and_font_scale() {
        let density = Density::new(2.0, 1.25);
        assert_eq!(Sp(16.0).to_px(density), Px(40.0));
    }

    #[test]
    fn dp_arithmetic_matches_plain_float_arithmetic() {
        assert_eq!(Dp(4.0) + Dp(2.0), Dp(6.0));
        assert_eq!(Dp(4.0) - Dp(2.0), Dp(2.0));
        assert_eq!(Dp(4.0) * 2.5, Dp(10.0));
        assert_eq!(Dp(10.0) / 4.0, Dp(2.5));
        assert_eq!(-Dp(4.0), Dp(-4.0));
        assert_eq!(Dp(4.0).max(Dp(9.0)), Dp(9.0));
        assert_eq!(Dp(4.0).min(Dp(9.0)), Dp(4.0));
    }

    #[test]
    fn literal_and_extension_constructors_agree() {
        assert_eq!(Dp::from(16.0f32), Dp(16.0));
        assert_eq!(Dp::from(16), Dp(16.0));
        assert_eq!(16.0.dp(), Dp(16.0));
        assert_eq!(16.sp(), Sp(16.0));
        assert_eq!(16.px(), Px(16.0));
    }

    /// `impl Into<Dp>` is what lets a widget/modifier parameter accept both
    /// a bare literal and an explicit `.dp()` call; this exercises exactly
    /// that generic boundary rather than the concrete type.
    #[test]
    fn into_dp_accepts_a_bare_literal_and_an_explicit_conversion() {
        fn padding(value: impl Into<Dp>) -> Dp {
            value.into()
        }

        assert_eq!(padding(16.0), Dp(16.0));
        assert_eq!(padding(16), Dp(16.0));
        assert_eq!(padding(16.0.dp()), Dp(16.0));
    }
}
