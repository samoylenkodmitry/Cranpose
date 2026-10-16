//! `f32::floor`, `f32::ceil` and `f32::round` without a library call.
//!
//! A target with no instruction that rounds a float, as the ESP32-S3 and
//! baseline x86 are, calls `floorf`, `ceilf` and `roundf` for them, and
//! layout snapping and the software renderer place pixels with dozens of
//! them a frame. There these functions truncate through the integer
//! conversion such targets do have and step by the exact remainder. Every
//! result is the one the `f32` method gives, bit for bit, signed zeros,
//! infinities and NaN included. Targets that round in one instruction use
//! the method.

/// Whether the target rounds a float in one instruction.
const ROUNDS_IN_HARDWARE: bool = !cfg!(any(
    target_arch = "xtensa",
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
