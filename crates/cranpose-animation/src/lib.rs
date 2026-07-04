//! Animation system for Cranpose
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.

#![deny(unsafe_code)]
#![allow(non_snake_case)]

pub mod animation;
pub mod color;
pub mod decay_spec;

// Re-export animation system
pub use animation::*;
pub use color::animateColorAsState;
pub use decay_spec::{FlingCalculator, FlingInfo, FloatDecayAnimationSpec, SplineBasedDecaySpec};

pub mod prelude {
    pub use crate::animation::{
        animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, spring, tween,
        Animatable, AnimationSpec, AnimationType, Easing, InfiniteRepeatableSpec,
        InfiniteTransition, Lerp, RepeatMode, Spring, SpringSpec, StartOffset, StartOffsetType,
    };
    pub use crate::color::animateColorAsState;
    pub use crate::decay_spec::{FlingCalculator, FloatDecayAnimationSpec, SplineBasedDecaySpec};
}
