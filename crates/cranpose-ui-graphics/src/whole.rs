//! Float rounding without a library call.
//!
//! A target with no instruction that rounds a float, as the ESP32-S3,
//! 32-bit ARM and baseline x86 are, calls `floorf`, `ceilf`, `roundf` and
//! `truncf` for the `f32` methods, and layout snapping and the renderers
//! place pixels with dozens of them a frame. There these functions truncate
//! through the integer conversion such targets do have and step by the
//! exact remainder. Wasm rounds halves only to even, so its `round`
//! truncates instead. Every result is the one the method gives, bit for
//! bit, signed zeros, infinities and NaN included. Targets that round in
//! one instruction use the method.

/// Whether the target rounds a float in one instruction.
const ROUNDS_IN_HARDWARE: bool = !cfg!(any(
    target_arch = "xtensa",
    target_arch = "arm",
    all(
        any(target_arch = "x86", target_arch = "x86_64"),
        not(target_feature = "sse4.1")
    )
));

/// From this magnitude up every `f32` is whole.
const WHOLE_FROM: f32 = 8_388_608.0;

/// `value` without its fraction, and the fraction, for a magnitude below
/// 2^23, where both are exact; `None` for a value that is whole already,
/// infinite or NaN.
#[inline(always)]
fn split(value: f32) -> Option<(f32, f32)> {
    // A NaN fails the comparison too.
    (value.abs() < WHOLE_FROM).then(|| {
        let whole = (value as i32) as f32;
        (whole, value - whole)
    })
}

/// `value.floor()`.
#[inline(always)]
pub fn floor(value: f32) -> f32 {
    if ROUNDS_IN_HARDWARE {
        return value.floor();
    }
    match split(value) {
        Some((whole, fraction)) if fraction < 0.0 => whole - 1.0,
        Some((whole, _)) => whole.copysign(value),
        None => value,
    }
}

/// `value.ceil()`.
#[inline(always)]
pub fn ceil(value: f32) -> f32 {
    if ROUNDS_IN_HARDWARE {
        return value.ceil();
    }
    match split(value) {
        Some((whole, fraction)) if fraction > 0.0 => whole + 1.0,
        Some((whole, _)) => whole.copysign(value),
        None => value,
    }
}

/// `value.round()`: halves away from zero.
#[inline(always)]
pub fn round(value: f32) -> f32 {
    if cfg!(target_arch = "wasm32") {
        // The float just below one half carries a fraction of a half or
        // more over the next whole number and no smaller one.
        return (value + 0.499_999_97_f32.copysign(value)).trunc();
    }
    if ROUNDS_IN_HARDWARE {
        return value.round();
    }
    match split(value) {
        Some((whole, fraction)) if fraction >= 0.5 => whole + 1.0,
        Some((whole, fraction)) if fraction <= -0.5 => whole - 1.0,
        Some((whole, _)) => whole.copysign(value),
        None => value,
    }
}

/// `value.trunc()`.
#[inline(always)]
pub fn trunc(value: f32) -> f32 {
    if ROUNDS_IN_HARDWARE {
        return value.trunc();
    }
    match split(value) {
        Some((whole, _)) => whole.copysign(value),
        None => value,
    }
}

/// `value.fract()`: `value` less its [`trunc`].
#[inline]
pub fn fract(value: f32) -> f32 {
    value - trunc(value)
}

// From this magnitude up the whole part of an `f64` does not fit `i32`.
const I32_FROM: f64 = 2_147_483_648.0;

/// `value.round()` for an `f64`: halves away from zero.
#[inline(always)]
pub fn round_f64(value: f64) -> f64 {
    if cfg!(target_arch = "wasm32") {
        // As in `round`, with the `f64` just below one half.
        return (value + 0.499_999_999_999_999_94_f64.copysign(value)).trunc();
    }
    if ROUNDS_IN_HARDWARE || value.abs() >= I32_FROM || value.is_nan() {
        return value.round();
    }
    // Below 2^31 the whole part and the remainder are exact.
    let whole = f64::from(value as i32);
    let fraction = value - whole;
    if fraction >= 0.5 {
        whole + 1.0
    } else if fraction <= -0.5 {
        whole - 1.0
    } else {
        whole.copysign(value)
    }
}

/// `value` as a whole number, its fraction dropped toward zero: what a
/// pixel position or count rounded with [`floor`], [`ceil`] or [`round`]
/// holds. Saturates at the ends of `i32`; 0 for NaN.
#[inline(always)]
pub fn to_i32(value: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the conversion saturates, which is what a pixel position wants"
    )]
    let whole = value as i32;
    whole
}
