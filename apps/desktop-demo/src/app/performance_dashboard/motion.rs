// How the Performance tab moves.

use cranpose_animation::{spring, tween, AnimationType, Easing, Spring};

// Below this share of its opacity a faded part is not drawn.
pub(super) const GONE: f32 = 0.01;

// How a hover fades a line, a bar or a name.
pub(super) fn fade() -> AnimationType {
    tween(160, Easing::EaseOut)
}

// How a scale, a bar's length and a row's place settle at their targets.
pub(super) fn settle() -> AnimationType {
    spring(Spring::DampingRatioNoBouncy, Spring::StiffnessMediumLow)
}
