# Cranpose Animation

This crate provides animation values for Cranpose compositions.

## When to Use

Use this crate for animations driven by state. A new target updates the transition from the current value. Springs preserve velocity across target changes.

## Key Concepts

-   **`Animatable<T>`**: A low-level holder for the current value and velocity. Higher-level animation APIs build on this type.
-   **`AnimationSpec`**: Defines the behavior of an animation. Common types include:
    -   **`Spring`**: Physical simulation based on stiffness and damping ratio.
    -   **`Tween`**: Duration-based interpolation with an easing curve.
-   **`animate*AsState`**: Composable functions return a `State` with the current animation value. The float, color, dp, offset, size and rect variants use `animateValueAsState`. Custom values implement `SpringScalar` and `Lerp`.
-   **`Transition<S>`**: `updateTransition` creates a finite transition. Child animations use explicit target values for the same transition state. `transition.is_running()` stays `true` until all children reach their targets.

## Example: Interruptible Spring Animation

```rust
use cranpose_animation::{animateFloatAsState, spring, Spring};
use cranpose::prelude::*;

#[composable]
fn AnimatedBox(target_size: f32) {
    let size = animateFloatAsState(
        target_size,
        spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessLow),
        "box_size",
    );
    
    Box(
        Modifier::empty()
            .width(size.get())
            .height(size.get())
            .background(Color(0.1, 0.3, 0.9, 1.0)),
        BoxSpec::default(),
        || {},
    );
}
```

## Example: Infinite Transition

```rust
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose::prelude::*;

#[composable]
fn PulsingDot() {
    let transition = rememberInfiniteTransition("pulse");
    let alpha = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(900, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "pulse_alpha",
    );

    Box(
        Modifier::empty()
            .width(24.0)
            .height(24.0)
            .background(Color(0.2, 0.5, 0.9, alpha.get())),
        BoxSpec::default(),
        || {},
    );
}
```

## Example: Finite, State-Driven Transition

```rust
use cranpose::prelude::*;
use cranpose_animation::{updateTransition, AnimationSpec, AnimationType, Easing};

#[composable]
fn ExpandingCard(expanded: bool) {
    let transition = updateTransition(expanded, "card");
    let tween = AnimationType::Tween(AnimationSpec::tween(240, Easing::FastOutSlowInEasing));

    let height = transition.animateFloat(if expanded { 320.0 } else { 96.0 }, tween, "height");
    let tint = transition.animateColor(
        if expanded { Color(0.1, 0.1, 0.15, 1.0) } else { Color(0.9, 0.9, 0.95, 1.0) },
        tween,
        "tint",
    );

    Box(
        Modifier::empty().height(height.get()).background(tint.get()),
        BoxSpec::default(),
        || {},
    );
}
```
