//! Color animation for Cranpose
//!
//! Mirrors Jetpack Compose's `animateColorAsState` from
//! `androidx.compose.animation.SingleValueAnimation` by layering a [`Lerp`]
//! implementation for [`Color`] over the generic [`Animatable`] machinery.
//!
//! Note: This module uses camelCase for function names to maintain 1:1 API
//! parity with Jetpack Compose.

#![allow(non_snake_case)]

use crate::animation::{Animatable, AnimationType, Lerp, SpringScalar};
use cranpose_core::{with_current_composer, Owned, State};
use cranpose_ui_graphics::Color;

impl Lerp for Color {
    /// Linearly interpolate each RGBA channel, including alpha.
    ///
    /// Jetpack Compose's default `Color` lerp converts through the Oklab
    /// color space before interpolating. Cranpose colors carry no color-space
    /// information, so this implementation interpolates each channel linearly
    /// in the color's own (linear RGBA) space instead. For typical UI fades
    /// between colors in the same space this closely matches Compose's
    /// behavior; the results are clamped to `[0.0, 1.0]` per channel so
    /// overshooting springs still produce valid colors, matching Compose's
    /// gamut coercion.
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        Color(
            self.0.lerp(&target.0, fraction).clamp(0.0, 1.0),
            self.1.lerp(&target.1, fraction).clamp(0.0, 1.0),
            self.2.lerp(&target.2, fraction).clamp(0.0, 1.0),
            self.3.lerp(&target.3, fraction).clamp(0.0, 1.0),
        )
    }
}

impl SpringScalar for Color {
    /// Magnitude of the RGBA vector.
    ///
    /// Only a coarse scalar view: spring progress and settling for colors use
    /// the 4-channel overrides below, mirroring how Compose animates colors
    /// as `AnimationVector4D`.
    fn to_f32(&self) -> f32 {
        (self.0 * self.0 + self.1 * self.1 + self.2 * self.2 + self.3 * self.3).sqrt()
    }

    /// Progress of `current` along the 4D line from `start` to `target`,
    /// computed as a vector projection so all channels contribute.
    fn spring_progress(start: &Self, target: &Self, current: &Self) -> f32 {
        let delta = [
            target.0 - start.0,
            target.1 - start.1,
            target.2 - start.2,
            target.3 - start.3,
        ];
        let len_sq: f32 = delta.iter().map(|d| d * d).sum();
        if len_sq < f32::EPSILON {
            1.0
        } else {
            let travelled = (current.0 - start.0) * delta[0]
                + (current.1 - start.1) * delta[1]
                + (current.2 - start.2) * delta[2]
                + (current.3 - start.3) * delta[3];
            travelled / len_sq
        }
    }

    /// Euclidean distance across all four channels.
    fn is_near_target(current: &Self, target: &Self, threshold: f32) -> bool {
        let dr = current.0 - target.0;
        let dg = current.1 - target.1;
        let db = current.2 - target.2;
        let da = current.3 - target.3;
        (dr * dr + dg * dg + db * db + da * da).sqrt() < threshold
    }
}

/// Fire-and-forget color animation. Returns a [`State`] whose value is
/// updated by animations towards the provided `target` whenever `target`
/// changes.
///
/// Mirrors Jetpack Compose:
/// `animateColorAsState(targetValue, animationSpec, label)`.
///
/// The interpolation happens linearly per RGBA channel, including alpha (see
/// [`Lerp`] for [`Color`] for how this relates to Compose's Oklab lerp).
pub fn animateColorAsState(target: Color, animation: AnimationType, label: &str) -> State<Color> {
    let _ = label;
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let anim: Owned<Animatable<Color>> = composer.remember(|| Animatable::new(target, runtime));
        anim.update(|animatable| {
            let is_new_target = animatable.target() != target;
            let is_new_animation = animatable.animation_type() != animation;
            if is_new_target || is_new_animation {
                animatable.animateTo(target, animation);
            }
        });
        anim.with(|animatable| animatable.state())
    })
}

#[cfg(test)]
#[path = "tests/color_tests.rs"]
mod tests;
