//! Animation over Cranpose's density/scale unit types.
//!
//! Layers [`Lerp`] and [`SpringScalar`] onto [`Dp`] and [`Sp`] so they plug
//! into the same generic [`animateValueAsState`] core that
//! [`animateFloatAsState`](crate::animateFloatAsState) and
//! [`animateColorAsState`](crate::animateColorAsState) already use.
//!
//! Note: This module uses camelCase for function names to maintain 1:1 API
//! parity with Jetpack Compose.

#![allow(non_snake_case)]

use cranpose_core::State;
use cranpose_ui_graphics::{Dp, Sp};

use crate::animation::{AnimationType, Lerp, SpringScalar, animateValueAsState};

impl Lerp for Dp {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Dp(self.0.lerp(&target.0, fraction))
    }
}

impl SpringScalar for Dp {
    const DIMENSIONS: usize = 1;

    fn dimension(&self, _index: usize) -> f32 {
        self.0
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Dp(dimensions[0])
    }
}

impl Lerp for Sp {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Sp(self.0.lerp(&target.0, fraction))
    }
}

impl SpringScalar for Sp {
    const DIMENSIONS: usize = 1;

    fn dimension(&self, _index: usize) -> f32 {
        self.0
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Sp(dimensions[0])
    }
}

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
