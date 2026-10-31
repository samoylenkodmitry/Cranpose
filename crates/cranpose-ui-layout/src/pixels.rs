//! Snapping lengths to the device pixel grid the way Compose does.
use cranpose_ui_graphics::whole;

/// A length moved onto the whole device pixel Compose would give it.
///
/// Compose's layout is integral — `Dp.roundToPx()` runs before anything is
/// measured and children are placed at an `IntOffset` — and Kotlin's
/// `roundToInt` sends an exact half **up**, not away from zero. Rust's
/// `f32::round` disagrees on exactly the negative halves, which is the case a
/// scroll offset reaches.
pub fn round_to_px(value: f32, density: f32) -> f32 {
    if density <= 0.0 || !density.is_finite() {
        return value;
    }
    round_to_grid(value, density)
}

/// [`round_to_px`] at a density known to be finite and positive.
pub fn round_to_grid(value: f32, density: f32) -> f32 {
    if !value.is_finite() {
        return value;
    }
    // At density 1 the scale and the division change no bit; a chip with no
    // float divider skips them.
    if density == 1.0 {
        return whole::floor(value + 0.5);
    }
    whole::floor(value * density + 0.5) / density
}

/// A length moved up onto the next whole device pixel, as Compose sizes
/// text: a layout's size is its paragraph's, `ceil`ed, so it never clips.
pub fn ceil_to_px(value: f32, density: f32) -> f32 {
    if density <= 0.0 || !density.is_finite() {
        return value;
    }
    ceil_to_grid(value, density)
}

/// [`ceil_to_px`] at a density known to be finite and positive.
pub fn ceil_to_grid(value: f32, density: f32) -> f32 {
    if !value.is_finite() {
        return value;
    }
    if density == 1.0 {
        return whole::ceil(value);
    }
    whole::ceil(value * density) / density
}

#[cfg(test)]
#[path = "tests/pixels_tests.rs"]
mod tests;
