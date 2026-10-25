//! Animation over Cranpose's density/scale unit types.
//!
//! Layers [`Lerp`] and [`SpringScalar`] onto [`Dp`] and [`Sp`] so they plug
//! into the same generic [`animateValueAsState`] core that
//! [`animateFloatAsState`](crate::animateFloatAsState) and
//! [`animateColorAsState`](crate::animateColorAsState) already use.
//!
//! Note: This module uses camelCase for function names to maintain 1:1 API
//! parity with Jetpack Compose.

#![expect(non_snake_case, reason = "names follow Compose's casing")]

use cranpose_core::State;
use cranpose_ui_graphics::{Dp, Sp};

use crate::animation::{AnimationType, Lerp, SpringScalar, animateValueAsState};

macro_rules! impl_length_animation {
    ($($unit:ty),*) => {$(
        impl Lerp for $unit {
            fn lerp(&self, target: &Self, fraction: f32) -> Self {
                Self(self.0.lerp(&target.0, fraction))
            }
        }

        impl SpringScalar for $unit {
            const DIMENSIONS: usize = 1;

            fn dimension(&self, _index: usize) -> f32 {
                self.0
            }

            fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
                Self(dimensions[0])
            }
        }
    )*};
}

impl_length_animation!(Dp, Sp);

/// Fire-and-forget animation of a density-independent length.
///
/// Mirrors Jetpack Compose: `animateDpAsState(targetValue, animationSpec, label)`.
#[track_caller]
pub fn animateDpAsState(target: Dp, animation: AnimationType, label: &str) -> State<Dp> {
    animateValueAsState(target, animation, label)
}

#[cfg(test)]
#[path = "tests/unit_animation_tests.rs"]
mod tests;
