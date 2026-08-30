# Cranpose Animation

A physics-based animation library designed for the Cranpose composition model.

## When to Use

Use this crate to create smooth, interruptible animations. Unlike traditional timeline-based animation systems, Cranpose animations are driven by state changes. When a target value changes, the animation system automatically calculates the transition from the current value to the new target, maintaining velocity and continuity.

## Key Concepts

-   **`Animatable<T, V>`**: A low-level value holder that tracks the current value and velocity. It is the primitive used to build higher-level animation APIs.
-   **`AnimationSpec`**: Defines the behavior of an animation. Common types include:
    -   **`Spring`**: Physical simulation based on stiffness and damping ratio.
    -   **`Tween`**: Duration-based interpolation with an easing curve.
-   **`animate*AsState`**: Composable functions that subscribe to a target value and return a `State` object representing the current animated value. `animateFloatAsState` and `animateColorAsState` are joined by `animateDpAsState`, `animateOffsetAsState`, `animateSizeAsState` and `animateRectAsState`, all built over the generic `animateValueAsState` and the `SpringScalar` vector-converter core -- any type that decomposes into a fixed-size float vector (see `SpringScalar`/`Lerp`) gets a specialization for free.
-   **`Transition<S>`**: A finite, state-driven animation obtained from `updateTransition`. Multiple child animations (`transition.animateFloat { }`, `.animateDp { }`, `.animateColor { }`, or the generic `.animateValue { }`) each derive their own target from the same state and run in lockstep; `transition.is_running()` is `true` until every child has settled.

## Example: Interruptible Spring Animation

```rust
use cranpose::prelude::*;

#[composable]
fn AnimatedBox(target_size: f32) {
    // animateFloatAsState automatically handles interruptions.
    // If target_size changes while animating, it will seamlessly retarget
    // preserving current velocity.
    let size = animateFloatAsState(
        target_size, 
        Some(spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessLow))
    );
    
    Box(
        Modifier
            .size(size.value())
            .background(Color::Blue)
    );
}
```

## Example: Infinite Transition

```rust
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_ui::*;

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
            .background(Color(0.2, 0.5, 0.9, alpha.value())),
        BoxSpec::default(),
        || {},
    );
}
```

## Example: Finite, State-Driven Transition

```rust
use cranpose_animation::{updateTransition, AnimationSpec, AnimationType, Easing};
use cranpose_ui_graphics::Color;

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

    // transition.is_running() stays true until both children have settled,
    // and flipping `expanded` again mid-animation retargets in place rather
    // than snapping.
    Box(
        Modifier::empty().height(height.value()).background(tint.value()),
        BoxSpec::default(),
        || {},
    );
}
```
