//! AnimatedVisibility composable
//!
//! Mirrors Jetpack Compose's `AnimatedVisibility` from
//! `androidx.compose.animation.AnimatedVisibility`, with a minimal set of
//! enter/exit transitions: `fade_in`/`fade_out` and
//! `slide_in_vertically`/`slide_out_vertically`.

#![allow(non_snake_case)]

use cranpose_animation::AnimationType;

use crate::{
    composable,
    modifier::{GraphicsLayer, Modifier},
    widgets::{
        box_widget::{Box, BoxSpec},
        crossfade::animate_float_with_initial,
    },
};

/// Progress below which exiting content is considered fully hidden and can
/// leave the composition.
const VISIBILITY_PROGRESS_EPSILON: f32 = 0.001;

/// Defines how an [`AnimatedVisibility`] content appears.
///
/// Mirrors Jetpack Compose's `EnterTransition`. Transitions are combined
/// with `+`, like Compose's `fadeIn() + slideInVertically()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnterTransition {
    fade: bool,
    slide_vertical_fraction: Option<f32>,
    animation: Option<AnimationType>,
}

impl EnterTransition {
    /// An empty enter transition: content appears without any effect.
    ///
    /// Mirrors Jetpack Compose: `EnterTransition.None`.
    pub fn none() -> Self {
        Self {
            fade: false,
            slide_vertical_fraction: None,
            animation: None,
        }
    }

    /// Use a custom animation spec for this transition. In Compose the spec
    /// is a parameter of each transition factory (e.g.
    /// `fadeIn(animationSpec = tween(300))`); here it is a builder method:
    /// `fade_in().with_animation(tween(300, Easing::LinearEasing))`.
    pub fn with_animation(mut self, animation: AnimationType) -> Self {
        self.animation = Some(animation);
        self
    }

    pub(crate) fn has_fade(&self) -> bool {
        self.fade
    }

    pub(crate) fn slide_vertical_fraction(&self) -> Option<f32> {
        self.slide_vertical_fraction
    }

    pub(crate) fn animation(&self) -> AnimationType {
        self.animation.unwrap_or_default()
    }
}

impl std::ops::Add for EnterTransition {
    type Output = Self;

    /// Combines two enter transitions, mirroring Compose's
    /// `EnterTransition.plus`: the resulting transition applies every effect
    /// of both operands. The left-hand animation spec wins when both sides
    /// carry one.
    fn add(self, other: Self) -> Self {
        Self {
            fade: self.fade || other.fade,
            slide_vertical_fraction: self
                .slide_vertical_fraction
                .or(other.slide_vertical_fraction),
            animation: self.animation.or(other.animation),
        }
    }
}

/// Defines how an [`AnimatedVisibility`] content disappears.
///
/// Mirrors Jetpack Compose's `ExitTransition`. Transitions are combined
/// with `+`, like Compose's `fadeOut() + slideOutVertically()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitTransition {
    fade: bool,
    slide_vertical_fraction: Option<f32>,
    animation: Option<AnimationType>,
}

impl ExitTransition {
    /// An empty exit transition: content disappears without any effect.
    ///
    /// Mirrors Jetpack Compose: `ExitTransition.None`.
    pub fn none() -> Self {
        Self {
            fade: false,
            slide_vertical_fraction: None,
            animation: None,
        }
    }

    /// Use a custom animation spec for this transition (see
    /// [`EnterTransition::with_animation`]).
    pub fn with_animation(mut self, animation: AnimationType) -> Self {
        self.animation = Some(animation);
        self
    }

    pub(crate) fn has_fade(&self) -> bool {
        self.fade
    }

    pub(crate) fn slide_vertical_fraction(&self) -> Option<f32> {
        self.slide_vertical_fraction
    }

    pub(crate) fn animation(&self) -> AnimationType {
        self.animation.unwrap_or_default()
    }
}

impl std::ops::Add for ExitTransition {
    type Output = Self;

    /// Combines two exit transitions, mirroring Compose's
    /// `ExitTransition.plus`: the resulting transition applies every effect
    /// of both operands. The left-hand animation spec wins when both sides
    /// carry one.
    fn add(self, other: Self) -> Self {
        Self {
            fade: self.fade || other.fade,
            slide_vertical_fraction: self
                .slide_vertical_fraction
                .or(other.slide_vertical_fraction),
            animation: self.animation.or(other.animation),
        }
    }
}

/// Fades in the content of an [`AnimatedVisibility`], from transparent to
/// fully opaque.
///
/// Mirrors Jetpack Compose: `fadeIn()`.
pub fn fade_in() -> EnterTransition {
    EnterTransition {
        fade: true,
        ..EnterTransition::none()
    }
}

/// Fades out the content of an [`AnimatedVisibility`], from fully opaque to
/// transparent.
///
/// Mirrors Jetpack Compose: `fadeOut()`.
pub fn fade_out() -> ExitTransition {
    ExitTransition {
        fade: true,
        ..ExitTransition::none()
    }
}

/// Slides in the content vertically from `initial_offset_fraction` of its
/// own height (negative values start above the resting position, mirroring
/// Compose's default of `-fullHeight / 2`).
///
/// Mirrors Jetpack Compose: `slideInVertically { fullHeight -> ... }`, with
/// the offset expressed as a fraction of the content height instead of a
/// lambda over the measured height.
pub fn slide_in_vertically(initial_offset_fraction: f32) -> EnterTransition {
    EnterTransition {
        slide_vertical_fraction: Some(initial_offset_fraction),
        ..EnterTransition::none()
    }
}

/// Slides out the content vertically towards `target_offset_fraction` of
/// its own height.
///
/// Mirrors Jetpack Compose: `slideOutVertically { fullHeight -> ... }`, with
/// the offset expressed as a fraction of the content height instead of a
/// lambda over the measured height.
pub fn slide_out_vertically(target_offset_fraction: f32) -> ExitTransition {
    ExitTransition {
        slide_vertical_fraction: Some(target_offset_fraction),
        ..ExitTransition::none()
    }
}

/// Animates the appearance and disappearance of its content when `visible`
/// changes.
///
/// Mirrors Jetpack Compose:
/// `AnimatedVisibility(visible, enter = ..., exit = ...) { ... }`.
///
/// Compose semantics: content that is visible on first composition appears
/// without an animation; when `visible` turns `false` the content stays
/// composed for the whole exit transition and leaves the composition once it
/// completes; toggling `visible` mid-transition retargets the running
/// animation from its current value.
///
/// Divergence from Compose: enter and exit each drive a single shared
/// progress (instead of one `Transition` animation per effect), so all
/// effects of a combined transition share one animation spec. Exiting with
/// `ExitTransition::none()` still holds the content for the duration of the
/// exit spec before removal.
#[composable]
pub fn AnimatedVisibility<F>(
    visible: bool,
    enter: EnterTransition,
    exit: ExitTransition,
    content: F,
) where
    F: FnMut() + 'static,
{
    let target = if visible { 1.0 } else { 0.0 };
    let animation = if visible {
        enter.animation()
    } else {
        exit.animation()
    };
    // First composition seeds the progress at the target so an initially
    // visible content appears without an enter animation, like Compose.
    let progress_state = animate_float_with_initial(target, target, animation);
    // Reading here subscribes this recompose scope: each animation frame
    // re-evaluates whether the exiting content can leave the composition.
    let progress = progress_state.value();

    let composed = visible || progress > VISIBILITY_PROGRESS_EPSILON;
    if composed {
        let fade = if visible {
            enter.has_fade()
        } else {
            exit.has_fade()
        };
        let slide_fraction = if visible {
            enter.slide_vertical_fraction()
        } else {
            exit.slide_vertical_fraction()
        };

        let alpha = if fade { progress.clamp(0.0, 1.0) } else { 1.0 };
        let mut modifier = Modifier::empty().graphics_layer_value(GraphicsLayer {
            alpha,
            ..Default::default()
        });
        if let Some(fraction) = slide_fraction {
            modifier = modifier.offset_fraction(0.0, fraction * (1.0 - progress));
        }

        Box(modifier, BoxSpec::new(), content);
    }
}
