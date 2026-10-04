# cranpose-animation

`cranpose-animation` provides spring, tween, decay, and state-driven transition
APIs for Cranpose. The crate owns animation math and frame callbacks; UI widgets
use the same types as app code.

Depend directly on `cranpose-animation` for animation primitives or custom
widgets. UI applications also use the [`cranpose` facade](https://docs.rs/cranpose/latest/cranpose/)
for composition and host setup. The feature set is empty.

## Spring step

`advance_spring` computes one spring step from the current value, velocity,
target, damping ratio, stiffness, and elapsed seconds:

```rust
use cranpose_animation::{Spring, advance_spring};

let (position, velocity) = advance_spring(
    0.0,
    0.0,
    100.0,
    Spring::DampingRatioNoBouncy,
    Spring::StiffnessMedium,
    1.0 / 60.0,
);

assert!(position > 0.0);
assert!(velocity > 0.0);
```

Inside a composable, `animateFloatAsState` follows each new target and returns
a reactive `State<f32>`. `updateTransition` coordinates several values from one
state. `Animatable<T>` supports imperative targets and release velocity. Types
with one to four float dimensions can implement `SpringScalar` and `Lerp`.

- [API reference on docs.rs](https://docs.rs/cranpose-animation/latest/cranpose_animation/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-animation)
