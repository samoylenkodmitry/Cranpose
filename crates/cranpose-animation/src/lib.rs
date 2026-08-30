//! Animation system for Cranpose
//!
//! This crate provides animation primitives including tweens, springs, and easing functions.

#![allow(non_snake_case)]

pub mod animation;
pub mod color;
pub mod decay_spec;
pub mod geometry_animation;
#[cfg(test)]
pub(crate) mod test_support;
pub mod transition;
pub mod unit_animation;

pub use animation::*;
pub use color::animateColorAsState;
pub use decay_spec::{
    ExponentialDecaySpec, FloatDecayAnimationSpec, IOS_DECELERATION_RATE_FAST,
    IOS_DECELERATION_RATE_NORMAL,
};
pub use geometry_animation::{animateOffsetAsState, animateRectAsState, animateSizeAsState};
pub use transition::{Transition, updateTransition};
pub use unit_animation::animateDpAsState;

pub mod prelude {
    pub use crate::{
        animation::{
            Animatable, AnimationSpec, AnimationType, Easing, InfiniteRepeatableSpec,
            InfiniteTransition, Lerp, RepeatMode, Spring, SpringSpec, StartOffset, StartOffsetType,
            animateFloatAsState, animateValueAsState, infiniteRepeatable,
            rememberInfiniteTransition, spring, tween,
        },
        color::animateColorAsState,
        decay_spec::{ExponentialDecaySpec, FloatDecayAnimationSpec},
        geometry_animation::{animateOffsetAsState, animateRectAsState, animateSizeAsState},
        transition::{Transition, updateTransition},
        unit_animation::animateDpAsState,
    };
}
