//! Enter and exit transitions, and `AnimatedVisibility`.
//!
//! Mirrors Jetpack Compose's `EnterTransition`/`ExitTransition` from
//! `androidx.compose.animation`: fades, slides along either axis by a
//! fraction of the content's own size or across the container's edge, and
//! scales, combined with `+`.

use cranpose_animation::AnimationType;
use cranpose_ui_graphics::Size;

use crate::{
    composable,
    modifier::GraphicsLayer,
    widgets::animated_content::{ContentTransform, SeekableAnimatedContent},
};

/// Which way content slides across the edge of an
/// [`AnimatedContent`](crate::AnimatedContent) — Compose's
/// `AnimatedContentTransitionScope.SlideDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideDirection {
    /// Toward the left edge.
    Left,
    /// Toward the right edge.
    Right,
    /// Toward the top edge.
    Up,
    /// Toward the bottom edge.
    Down,
    /// Toward the start edge: the left one, or the right one in a
    /// right-to-left layout.
    Start,
    /// Toward the end edge: the right one, or the left one in a
    /// right-to-left layout.
    End,
}

impl SlideDirection {
    fn is_vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }

    /// Whether the direction points toward smaller coordinates.
    fn is_backward(self, rtl: bool) -> bool {
        match self {
            Self::Left | Self::Up => true,
            Self::Right | Self::Down => false,
            Self::Start => !rtl,
            Self::End => rtl,
        }
    }
}

/// How far content slides along one axis at full strength.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slide {
    /// A fraction of the content's own extent.
    Fraction(f32),
    /// Across the container's edge: content that comes starts just past the
    /// edge it comes from, and content that goes ends just past the edge it
    /// heads to.
    Container {
        towards: SlideDirection,
        entering: bool,
    },
}

impl Slide {
    /// The offset of content whose extent is `own` in a container of extent
    /// `container`, where content rests at the start.
    fn offset(self, own: f32, container: f32, rtl: bool) -> f32 {
        match self {
            Self::Fraction(fraction) => fraction * own,
            Self::Container { towards, entering } => {
                if entering == towards.is_backward(rtl) {
                    container
                } else {
                    -own
                }
            }
        }
    }

    fn uses_direction(self) -> bool {
        matches!(
            self,
            Self::Container {
                towards: SlideDirection::Start | SlideDirection::End,
                ..
            }
        )
    }
}

/// What container slides measure from: the container's size, and whether
/// its layout reads right to left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ContainerFrame {
    pub(crate) size: Size,
    pub(crate) rtl: bool,
}

/// What a transition does to its content at full strength; progress `1.0`
/// shows the content at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TransitionEffects {
    fade: bool,
    slide_x: Option<Slide>,
    slide_y: Option<Slide>,
    scale: Option<f32>,
}

impl TransitionEffects {
    fn merged(self, other: Self) -> Self {
        Self {
            fade: self.fade || other.fade,
            slide_x: self.slide_x.or(other.slide_x),
            slide_y: self.slide_y.or(other.slide_y),
            scale: self.scale.or(other.scale),
        }
    }

    /// Whether a slide toward the start or the end needs the layout
    /// direction.
    pub(crate) fn uses_direction(self) -> bool {
        self.slide_x.is_some_and(Slide::uses_direction)
    }

    /// The layer that shows content of `size` in `container` at `progress`
    /// of the way to rest.
    pub(crate) fn layer(
        self,
        progress: f32,
        size: Size,
        container: ContainerFrame,
    ) -> GraphicsLayer {
        let away = 1.0 - progress;
        let offset = |slide: Option<Slide>, own: f32, extent: f32| {
            slide.map_or(0.0, |slide| slide.offset(own, extent, container.rtl) * away)
        };
        GraphicsLayer {
            alpha: if self.fade {
                progress.clamp(0.0, 1.0)
            } else {
                1.0
            },
            translation_x: offset(self.slide_x, size.width, container.size.width),
            translation_y: offset(self.slide_y, size.height, container.size.height),
            scale: self
                .scale
                .map_or(1.0, |scale| scale + (1.0 - scale) * progress),
            ..GraphicsLayer::default()
        }
    }
}

/// Defines how content appears.
///
/// Mirrors Jetpack Compose's `EnterTransition`. Transitions are combined
/// with `+`, like Compose's `fadeIn() + slideInVertically()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnterTransition {
    effects: TransitionEffects,
    animation: Option<AnimationType>,
}

impl EnterTransition {
    /// An empty enter transition: content appears without any effect.
    ///
    /// Mirrors Jetpack Compose: `EnterTransition.None`.
    pub fn none() -> Self {
        Self {
            effects: TransitionEffects::default(),
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

    /// This transition for the content that comes in, and `exit` for the
    /// content that goes, as an [`AnimatedContent`](crate::AnimatedContent)
    /// change. Compose's `enter togetherWith exit`.
    pub fn together_with(self, exit: ExitTransition) -> ContentTransform {
        ContentTransform {
            enter: self,
            exit,
            size_transform: None,
        }
    }

    pub(crate) fn effects(&self) -> TransitionEffects {
        self.effects
    }

    pub(crate) fn animation(&self) -> AnimationType {
        self.animation.unwrap_or_default()
    }
}

impl std::ops::Add for EnterTransition {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            effects: self.effects.merged(other.effects),
            animation: self.animation.or(other.animation),
        }
    }
}

/// Defines how content disappears.
///
/// Mirrors Jetpack Compose's `ExitTransition`. Transitions are combined
/// with `+`, like Compose's `fadeOut() + slideOutVertically()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExitTransition {
    effects: TransitionEffects,
    animation: Option<AnimationType>,
}

impl ExitTransition {
    /// An empty exit transition: content disappears without any effect.
    ///
    /// Mirrors Jetpack Compose: `ExitTransition.None`.
    pub fn none() -> Self {
        Self {
            effects: TransitionEffects::default(),
            animation: None,
        }
    }

    /// Use a custom animation spec for this transition (see
    /// [`EnterTransition::with_animation`]).
    pub fn with_animation(mut self, animation: AnimationType) -> Self {
        self.animation = Some(animation);
        self
    }

    pub(crate) fn effects(&self) -> TransitionEffects {
        self.effects
    }

    pub(crate) fn animation(&self) -> AnimationType {
        self.animation.unwrap_or_default()
    }
}

impl std::ops::Add for ExitTransition {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            effects: self.effects.merged(other.effects),
            animation: self.animation.or(other.animation),
        }
    }
}

/// Fades in the content of an [`AnimatedVisibility`], from transparent to
/// fully opaque.
///
/// Mirrors Jetpack Compose: `fadeIn()`.
pub fn fade_in() -> EnterTransition {
    enter(TransitionEffects {
        fade: true,
        ..TransitionEffects::default()
    })
}

/// Fades out the content of an [`AnimatedVisibility`], from fully opaque to
/// transparent.
///
/// Mirrors Jetpack Compose: `fadeOut()`.
pub fn fade_out() -> ExitTransition {
    exit(TransitionEffects {
        fade: true,
        ..TransitionEffects::default()
    })
}

/// Slides in the content vertically from `initial_offset_fraction` of its
/// own height (negative values start above the resting position, mirroring
/// Compose's default of `-fullHeight / 2`).
///
/// Mirrors Jetpack Compose: `slideInVertically { fullHeight -> ... }`, with
/// the offset expressed as a fraction of the content height instead of a
/// lambda over the measured height.
pub fn slide_in_vertically(initial_offset_fraction: f32) -> EnterTransition {
    enter(TransitionEffects {
        slide_y: Some(Slide::Fraction(initial_offset_fraction)),
        ..TransitionEffects::default()
    })
}

/// Slides out the content vertically towards `target_offset_fraction` of
/// its own height.
///
/// Mirrors Jetpack Compose: `slideOutVertically { fullHeight -> ... }`, with
/// the offset expressed as a fraction of the content height instead of a
/// lambda over the measured height.
pub fn slide_out_vertically(target_offset_fraction: f32) -> ExitTransition {
    exit(TransitionEffects {
        slide_y: Some(Slide::Fraction(target_offset_fraction)),
        ..TransitionEffects::default()
    })
}

/// Slides in the content horizontally from `initial_offset_fraction` of its
/// own width: `1.0` starts one width to the right, `-1.0` one to the left.
///
/// Mirrors Jetpack Compose: `slideInHorizontally { fullWidth -> ... }`.
pub fn slide_in_horizontally(initial_offset_fraction: f32) -> EnterTransition {
    enter(TransitionEffects {
        slide_x: Some(Slide::Fraction(initial_offset_fraction)),
        ..TransitionEffects::default()
    })
}

/// Slides out the content horizontally towards `target_offset_fraction` of
/// its own width.
///
/// Mirrors Jetpack Compose: `slideOutHorizontally { fullWidth -> ... }`.
pub fn slide_out_horizontally(target_offset_fraction: f32) -> ExitTransition {
    exit(TransitionEffects {
        slide_x: Some(Slide::Fraction(target_offset_fraction)),
        ..TransitionEffects::default()
    })
}

/// Slides in the content across the container's edge `towards` a
/// direction: toward [`SlideDirection::Left`] it comes from just past the
/// container's right edge. The container is the
/// [`AnimatedContent`](crate::AnimatedContent) it changes in.
///
/// Mirrors Jetpack Compose: `slideIntoContainer(towards)`, with the full
/// slide of Compose's default `initialOffset`.
pub fn slide_into_container(towards: SlideDirection) -> EnterTransition {
    enter(container_slide(towards, true))
}

/// Slides out the content across the container's edge `towards` a
/// direction: toward [`SlideDirection::Left`] it ends just past the
/// container's left edge.
///
/// Mirrors Jetpack Compose: `slideOutOfContainer(towards)`, with the full
/// slide of Compose's default `targetOffset`.
pub fn slide_out_of_container(towards: SlideDirection) -> ExitTransition {
    exit(container_slide(towards, false))
}

fn container_slide(towards: SlideDirection, entering: bool) -> TransitionEffects {
    let slide = Some(Slide::Container { towards, entering });
    if towards.is_vertical() {
        TransitionEffects {
            slide_y: slide,
            ..TransitionEffects::default()
        }
    } else {
        TransitionEffects {
            slide_x: slide,
            ..TransitionEffects::default()
        }
    }
}

/// Scales in the content from `initial_scale` of its size, about its
/// centre.
///
/// Mirrors Jetpack Compose: `scaleIn(initialScale)`.
pub fn scale_in(initial_scale: f32) -> EnterTransition {
    enter(TransitionEffects {
        scale: Some(initial_scale),
        ..TransitionEffects::default()
    })
}

/// Scales out the content towards `target_scale` of its size, about its
/// centre.
///
/// Mirrors Jetpack Compose: `scaleOut(targetScale)`.
pub fn scale_out(target_scale: f32) -> ExitTransition {
    exit(TransitionEffects {
        scale: Some(target_scale),
        ..TransitionEffects::default()
    })
}

fn enter(effects: TransitionEffects) -> EnterTransition {
    EnterTransition {
        effects,
        animation: None,
    }
}

fn exit(effects: TransitionEffects) -> ExitTransition {
    ExitTransition {
        effects,
        animation: None,
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
/// completes, keeping its size until then; toggling `visible`
/// mid-transition retargets the running animation from its current value.
/// The animation moves the content's layer only: nothing recomposes while it
/// runs.
///
/// Divergence from Compose: enter and exit each drive a single shared
/// progress (instead of one `Transition` animation per effect), so all
/// effects of a combined transition share one animation spec.
#[composable]
pub fn AnimatedVisibility<F>(
    visible: bool,
    enter: EnterTransition,
    exit: ExitTransition,
    mut content: F,
) where
    F: FnMut() + 'static,
{
    SeekableAnimatedContent(
        visible,
        None,
        move |_, &shown| {
            if shown {
                enter.together_with(ExitTransition::none().with_animation(enter.animation()))
            } else {
                EnterTransition::none()
                    .with_animation(exit.animation())
                    .together_with(exit)
            }
        },
        move |shown| {
            if shown {
                content();
            }
        },
    );
}
