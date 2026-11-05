//! Snapping lengths to the device pixel grid the way Compose does.
use crate::whole;

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
    // An infinite or NaN length comes out as it went in, with no test of
    // its own. At density 1 the scale and the division change no bit; a chip with no
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
    if density == 1.0 {
        return whole::ceil(value);
    }
    let scaled = value * density;
    let pixels = whole::ceil(scaled);
    // A length the ceiling leaves where it was is on the grid already, as an
    // infinite one is; a NaN stays NaN below.
    if scaled >= pixels {
        return value;
    }
    // A length on the grid, `n / density`, scales back to a hair over `n` at
    // most fractional densities; the pixel below still covers it.
    let below = (pixels - 1.0) / density;
    if below >= value {
        below
    } else {
        pixels / density
    }
}

#[cfg(test)]
#[path = "tests/pixels_tests.rs"]
mod tests;
