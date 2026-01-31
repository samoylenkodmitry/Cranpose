# Cranpose Animation

A physics-based animation library designed for the Cranpose composition model.

## When to Use

Use this crate to create smooth, interruptible animations. Unlike traditional timeline-based animation systems, Cranpose animations are driven by state changes. When a target value changes, the animation system automatically calculates the transition from the current value to the new target, maintaining velocity and continuity.

## Key Concepts

-   **`Animatable<T, V>`**: A low-level value holder that tracks the current value and velocity. It is the primitive used to build higher-level animation APIs.
-   **`AnimationSpec`**: Defines the behavior of an animation. Common types include:
    -   **`Spring`**: Physical simulation based on stiffness and damping ratio.
    -   **`Tween`**: Duration-based interpolation with an easing curve.
-   **`animate*AsState`**: Composable functions that subscribe to a target value and return a `State` object representing the current animated value.

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
