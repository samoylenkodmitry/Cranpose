//! Crossfade composable
//!
//! Mirrors Jetpack Compose's `Crossfade` from
//! `androidx.compose.animation.Crossfade`: an
//! [`AnimatedContent`](crate::AnimatedContent) whose contents fade.

use cranpose_animation::AnimationType;

use crate::{
    composable,
    widgets::{
        animated_content::{ContentSeek, SeekableAnimatedContent},
        animated_visibility::{fade_in, fade_out},
    },
};

/// Crossfade allows switching between two layouts with a crossfade
/// animation.
///
/// Mirrors Jetpack Compose:
/// `Crossfade(targetState, animationSpec) { state -> ... }`.
///
/// When `target_state` changes, the content for the previous state fades out
/// while the content for the new state fades in. Compose semantics are
/// preserved: both compositions stay alive during the transition, each
/// wrapped in an alpha graphics layer, and the outgoing content leaves the
/// composition once its fade-out completes. The content shown on first
/// composition appears without an animation, and retargeting mid-transition
/// keeps every in-flight content fading until it finishes.
///
/// `content` is invoked with each in-flight state, so it must not capture
/// the current target; use the provided value instead.
#[composable(no_skip)]
pub fn Crossfade<T, F>(target_state: T, animation: AnimationType, content: F)
where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    SeekableCrossfade(target_state, None, animation, content);
}

/// [`Crossfade`] that a gesture can move part of the way, as Compose's
/// `SeekableTransitionState` moves a transition. While `seek` is set, the
/// content for `seek.toward` shows beneath the target's at
/// `seek.fraction`, and the target's at the rest. When the gesture ends, the
/// target either becomes `seek.toward` or stays, and both contents animate
/// on from where the gesture left them, with `animation`.
#[composable(no_skip)]
pub fn SeekableCrossfade<T, F>(
    target_state: T,
    seek: Option<ContentSeek<T>>,
    animation: AnimationType,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    SeekableAnimatedContent(
        target_state,
        seek,
        move |_, _| {
            fade_in()
                .with_animation(animation)
                .together_with(fade_out().with_animation(animation))
        },
        content,
    );
}
