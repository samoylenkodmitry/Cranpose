//! Snapping lengths to the device pixel grid the way Compose does.

/// A length moved onto the whole device pixel Compose would give it.
///
/// Compose's layout is integral — `Dp.roundToPx()` runs before anything is
/// measured and children are placed at an `IntOffset` — and Kotlin's
/// `roundToInt` sends an exact half **up**, not away from zero. Rust's
/// `f32::round` disagrees on exactly the negative halves, which is the case a
/// scroll offset reaches.
pub fn round_to_px(value: f32, density: f32) -> f32 {
    if density <= 0.0 || !density.is_finite() || !value.is_finite() {
        return value;
    }
    // At density 1 the scale and the division change no bit; a chip with no
    // float divider skips them.
    if density == 1.0 {
        return (value + 0.5).floor();
    }
    (value * density + 0.5).floor() / density
}

/// A length moved up onto the next whole device pixel, as Compose sizes
/// text: a layout's size is its paragraph's, `ceil`ed, so it never clips.
pub fn ceil_to_px(value: f32, density: f32) -> f32 {
    if density <= 0.0 || !density.is_finite() || !value.is_finite() {
        return value;
    }
    if density == 1.0 {
        return value.ceil();
    }
    (value * density).ceil() / density
}

#[cfg(test)]
#[path = "tests/pixels_tests.rs"]
mod tests;
