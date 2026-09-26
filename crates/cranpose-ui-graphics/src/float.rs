//! Float guards for the per-primitive recording path, each one compare.
//!
//! On armv7 every float compare moves the FPSCR flags to the core with
//! `vmrs`, which stalls a Cortex-A53's pipeline. `f32::max`, `f32::clamp`
//! and `f32::is_finite` add a NaN or absolute-value test to each call, so a
//! watch recording thousands of arcs a frame spent a tenth of its main
//! thread in them. These return the same values for the operands the
//! recorder passes.

/// `value.max(floor)` for a `floor` that is not NaN: a NaN `value` gives
/// `floor`, as `f32::max` does.
#[inline(always)]
pub(crate) fn at_least(value: f32, floor: f32) -> f32 {
    if value > floor { value } else { floor }
}

/// `value.clamp(low, high)` for bounds that are not NaN with
/// `low <= high`: a NaN `value` stays NaN, as `f32::clamp` keeps it.
#[inline(always)]
pub(crate) fn within(value: f32, low: f32, high: f32) -> f32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// Whether every value is finite, in one compare: a NaN or an infinity
/// times zero is NaN, and a sum holding a NaN is never zero.
#[inline(always)]
pub(crate) fn all_finite<const N: usize>(values: [f32; N]) -> bool {
    values.iter().fold(0.0, |sum, value| sum + value * 0.0) == 0.0
}

#[cfg(test)]
#[path = "tests/float_tests.rs"]
mod tests;
