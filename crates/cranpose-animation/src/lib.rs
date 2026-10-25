#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod animation;
pub mod color;
pub mod decay_spec;
mod easing;
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
pub use easing::Easing;
pub use geometry_animation::{animateOffsetAsState, animateRectAsState, animateSizeAsState};
pub use transition::{Transition, updateTransition};
pub use unit_animation::animateDpAsState;

/// The animation API in one import.
pub mod prelude {
    pub use crate::{
        animation::{
            Animatable, AnimationSpec, AnimationType, InfiniteRepeatableSpec, InfiniteTransition,
            Lerp, RepeatMode, Spring, SpringSpec, StartOffset, StartOffsetType,
            animateFloatAsState, animateValueAsState, infiniteRepeatable,
            rememberInfiniteTransition, spring, tween,
        },
        color::animateColorAsState,
        decay_spec::{ExponentialDecaySpec, FloatDecayAnimationSpec},
        easing::Easing,
        geometry_animation::{animateOffsetAsState, animateRectAsState, animateSizeAsState},
        transition::{Transition, updateTransition},
        unit_animation::animateDpAsState,
    };
}
