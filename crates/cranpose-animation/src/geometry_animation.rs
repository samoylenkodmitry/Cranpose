//! Animation over Cranpose's 2D geometry types.
//!
//! Layers [`Lerp`] and [`SpringScalar`] onto [`Point`], [`Size`] and [`Rect`]
//! so they plug into the same generic [`animateValueAsState`] core as
//! [`animateFloatAsState`](crate::animateFloatAsState) and
//! [`animateColorAsState`](crate::animateColorAsState).
//!
//! Note: This module uses camelCase for function names to maintain 1:1 API
//! parity with Jetpack Compose.

#![allow(non_snake_case)]

use cranpose_core::State;
use cranpose_ui_graphics::{Point, Rect, Size};

use crate::animation::{AnimationType, Lerp, SpringScalar, animateValueAsState};

impl Lerp for Point {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Point::new(
            self.x.lerp(&target.x, fraction),
            self.y.lerp(&target.y, fraction),
        )
    }
}

impl SpringScalar for Point {
    const DIMENSIONS: usize = 2;

    fn dimension(&self, index: usize) -> f32 {
        match index {
            0 => self.x,
            _ => self.y,
        }
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Point::new(dimensions[0], dimensions[1])
    }
}

impl Lerp for Size {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Size::new(
            self.width.lerp(&target.width, fraction),
            self.height.lerp(&target.height, fraction),
        )
    }
}

impl SpringScalar for Size {
    const DIMENSIONS: usize = 2;

    fn dimension(&self, index: usize) -> f32 {
        match index {
            0 => self.width,
            _ => self.height,
        }
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Size::new(dimensions[0], dimensions[1])
    }
}

impl Lerp for Rect {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Rect {
            x: self.x.lerp(&target.x, fraction),
            y: self.y.lerp(&target.y, fraction),
            width: self.width.lerp(&target.width, fraction),
            height: self.height.lerp(&target.height, fraction),
        }
    }
}

impl SpringScalar for Rect {
    const DIMENSIONS: usize = 4;

    fn dimension(&self, index: usize) -> f32 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.width,
            _ => self.height,
        }
    }

    fn from_dimensions(dimensions: [f32; crate::animation::SPRING_MAX_DIMENSIONS]) -> Self {
        Rect {
            x: dimensions[0],
            y: dimensions[1],
            width: dimensions[2],
            height: dimensions[3],
        }
    }
}

/// Fire-and-forget animation of a 2D position.
///
/// Mirrors Jetpack Compose: `animateOffsetAsState(targetValue, animationSpec, label)`,
/// using Cranpose's own [`Point`] in place of Compose's `Offset`.
#[track_caller]
pub fn animateOffsetAsState(target: Point, animation: AnimationType, label: &str) -> State<Point> {
    animateValueAsState(target, animation, label)
}

/// Fire-and-forget animation of a 2D extent.
///
/// Mirrors Jetpack Compose: `animateSizeAsState(targetValue, animationSpec, label)`.
#[track_caller]
pub fn animateSizeAsState(target: Size, animation: AnimationType, label: &str) -> State<Size> {
    animateValueAsState(target, animation, label)
}

/// Fire-and-forget animation of a rectangle.
///
/// Mirrors Jetpack Compose: `animateRectAsState(targetValue, animationSpec, label)`.
#[track_caller]
pub fn animateRectAsState(target: Rect, animation: AnimationType, label: &str) -> State<Rect> {
    animateValueAsState(target, animation, label)
}

#[cfg(test)]
#[path = "tests/geometry_animation_tests.rs"]
mod tests;
