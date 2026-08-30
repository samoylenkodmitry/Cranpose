//! Color animation for Cranpose
//!
//! Mirrors Jetpack Compose's `animateColorAsState` from
//! `androidx.compose.animation.SingleValueAnimation` by layering a [`Lerp`]
//! implementation for [`Color`] over the generic [`crate::Animatable`] machinery.
//!
//! Note: This module uses camelCase for function names to maintain 1:1 API
//! parity with Jetpack Compose.

#![allow(non_snake_case)]

use cranpose_core::State;
use cranpose_ui_graphics::Color;

use crate::animation::{AnimationType, Lerp, SpringScalar, animateValueAsState};

impl Lerp for Color {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Color(
            self.0.lerp(&target.0, fraction).clamp(0.0, 1.0),
            self.1.lerp(&target.1, fraction).clamp(0.0, 1.0),
            self.2.lerp(&target.2, fraction).clamp(0.0, 1.0),
            self.3.lerp(&target.3, fraction).clamp(0.0, 1.0),
        )
    }
}

/// Colors spring as four independent channels, mirroring how Compose animates
/// `Color` as an `AnimationVector4D`. Channels are clamped back into `[0, 1]`
/// when the vector is rebuilt, so overshooting springs still produce valid
/// colors (Compose's gamut coercion).
impl SpringScalar for Color {
    const DIMENSIONS: usize = 4;

    fn dimension(&self, index: usize) -> f32 {
        match index {
            0 => self.0,
            1 => self.1,
            2 => self.2,
            _ => self.3,
        }
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Color(
            dimensions[0].clamp(0.0, 1.0),
            dimensions[1].clamp(0.0, 1.0),
            dimensions[2].clamp(0.0, 1.0),
            dimensions[3].clamp(0.0, 1.0),
        )
    }
}

/// Fire-and-forget color animation. Returns a [`State`] whose value is
/// updated by animations towards the provided `target` whenever `target`
/// changes.
///
/// Mirrors Jetpack Compose:
/// `animateColorAsState(targetValue, animationSpec, label)`.
///
/// The interpolation happens linearly per RGBA channel, including alpha (see
/// [`Lerp`] for [`Color`] for how this relates to Compose's Oklab lerp).
#[track_caller]
pub fn animateColorAsState(target: Color, animation: AnimationType, label: &str) -> State<Color> {
    animateValueAsState(target, animation, label)
}

#[cfg(test)]
#[path = "tests/color_tests.rs"]
mod tests;
