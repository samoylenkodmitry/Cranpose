//! Animation system for Cranpose
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.

#![allow(non_snake_case)]

pub mod animation;
pub mod color;
pub mod decay_spec;

pub use animation::*;
pub use color::animateColorAsState;
pub use decay_spec::{
    ExponentialDecaySpec, FloatDecayAnimationSpec, IOS_DECELERATION_RATE_FAST,
    IOS_DECELERATION_RATE_NORMAL,
};

pub mod prelude {
    pub use crate::{
        animation::{
            Animatable, AnimationSpec, AnimationType, Easing, InfiniteRepeatableSpec,
            InfiniteTransition, Lerp, RepeatMode, Spring, SpringSpec, StartOffset, StartOffsetType,
            animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, spring, tween,
        },
        color::animateColorAsState,
        decay_spec::{ExponentialDecaySpec, FloatDecayAnimationSpec},
    };
}
