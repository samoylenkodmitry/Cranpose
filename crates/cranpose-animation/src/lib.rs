//! Animation system for Cranpose
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.

#![allow(non_snake_case)]

pub mod animation;
pub mod decay_spec;

// Re-export animation system
pub use animation::*;
pub use decay_spec::{FlingCalculator, FlingInfo, FloatDecayAnimationSpec, SplineBasedDecaySpec};

pub mod prelude {
    pub use crate::animation::{
        animateFloatAsState, animateFloatAsStateWithSpec, infiniteRepeatable,
        rememberInfiniteTransition, Animatable, AnimationSpec, AnimationType, Easing,
        InfiniteRepeatableSpec, InfiniteTransition, Lerp, RepeatMode, SpringSpec, StartOffset,
        StartOffsetType,
    };
    pub use crate::decay_spec::{FlingCalculator, FloatDecayAnimationSpec, SplineBasedDecaySpec};
}
