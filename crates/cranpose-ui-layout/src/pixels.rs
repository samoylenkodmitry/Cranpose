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
    (value * density + 0.5).floor() / density
}

#[cfg(test)]
#[path = "tests/pixels_tests.rs"]
mod tests;
